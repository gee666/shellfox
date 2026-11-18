use super::super::{
    protocol::{Frame, GuestStatus, RecoveryEvidence},
    tunnel::Tunnel,
    WslConfig, WslError, WslGuestGuardian,
};
use std::{
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};
pub(super) struct Owner {
    config: WslConfig,
    pub tunnel: Tunnel,
    pub recovery: Option<RecoveryEvidence>,
    pub status: Option<GuestStatus>,
    pub ready: bool,
    pub closing: bool,
    zero: bool,
    activated: bool,
    next_id: u64,
}
impl Owner {
    pub fn new(config: WslConfig, tunnel: Tunnel) -> Self {
        Self {
            config,
            tunnel,
            recovery: None,
            status: None,
            ready: false,
            closing: false,
            zero: false,
            activated: false,
            next_id: 0,
        }
    }
    pub fn start(&mut self, boot: &str) -> Result<(), WslError> {
        self.tunnel.send(self.config.payload())?;
        let (anchor, endpoint) = match self.tunnel.next()? {
            Frame::Prepared { anchor, endpoint } => (anchor, endpoint),
            _ => return Err(WslError::Protocol),
        };
        anchor.validate()?;
        if anchor.boot_id != boot
            || anchor.marker != self.config.marker
            || endpoint != format!("{}/control.sock", self.config.state_directory)
        {
            return Err(WslError::Protocol);
        }
        self.recovery = Some(RecoveryEvidence {
            settings: self.config.settings.clone(),
            distro: self.config.distro.clone(),
            secret: self.config.secret.clone(),
            endpoint: endpoint.clone(),
            anchor,
            identity: None,
        });
        // All recoverable credentials exist before the guest can allocate a PTY.
        self.activated = true;
        self.tunnel.mark_launch();
        self.tunnel.send(serde_json::json!({"action":"activate"}))?;
        match self.tunnel.next()? {
            Frame::Ready {
                status,
                endpoint: returned,
            } if returned == endpoint => {
                self.accept_status(status)?;
                self.ready = !self.zero;
                Ok(())
            }
            frame => {
                self.accept(frame)?;
                Err(WslError::Unavailable)
            }
        }
    }
    fn accept_status(&mut self, status: GuestStatus) -> Result<(), WslError> {
        let recovery = self.recovery.as_mut().ok_or(WslError::Protocol)?;
        status.identity.validate()?;
        if !recovery.anchor.matches(&status.identity) {
            return Err(WslError::Protocol);
        }
        status.validate(recovery.identity.as_ref().unwrap_or(&status.identity))?;
        if recovery.identity.is_none() {
            recovery.identity = Some(status.identity.clone());
        }
        if status.cleanup_confirmed && status.owned == 0 {
            self.zero = true;
            self.ready = false;
            self.tunnel.confirm_zero();
        }
        if status.root_exit.is_some() {
            self.closing = true;
            self.ready = false;
        }
        self.status = Some(status);
        Ok(())
    }
    fn accept(&mut self, frame: Frame) -> Result<(), WslError> {
        match frame {
            Frame::Stopped { status } => self.accept_status(status),
            Frame::Reply { id, status } if id > 0 && id == self.next_id => {
                self.accept_status(status)
            }
            Frame::Aborted {
                anchor,
                cleanup_confirmed: true,
                owned: 0,
            } if self.recovery.as_ref().is_some_and(|r| r.anchor == anchor) => {
                self.zero = true;
                self.ready = false;
                self.tunnel.confirm_zero();
                Ok(())
            }
            Frame::Eof {} if self.zero => Ok(()),
            _ => Err(WslError::Protocol),
        }
    }
    pub fn poll(&mut self) -> Result<(), WslError> {
        if let Err(error) = self.tunnel.flush_acks() {
            if !self.zero {
                return Err(error);
            }
        }
        for _ in 0..8 {
            match self.tunnel.frames.try_recv() {
                Ok(Ok(frame)) => self.accept(frame)?,
                Ok(Err(error)) => {
                    if !self.zero {
                        return Err(error);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.zero {
                        break;
                    } else {
                        return Err(WslError::CleanupUnconfirmed);
                    }
                }
            }
        }
        Ok(())
    }
    fn close_attached(&mut self) -> Result<(), WslError> {
        self.next_id += 1;
        let id = self.next_id;
        self.tunnel
            .send(serde_json::json!({"action":"close","id":id}))?;
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            match self.tunnel.next_until(deadline)? {
                Frame::Reply { id: reply, status } if reply == id => {
                    self.accept_status(status)?;
                    return if self.zero {
                        Ok(())
                    } else {
                        Err(WslError::CleanupUnconfirmed)
                    };
                }
                frame => {
                    self.accept(frame)?;
                    if self.zero {
                        return Ok(());
                    }
                }
            }
        }
    }
    pub fn close_guest(&mut self) -> Result<(), WslError> {
        self.ready = false;
        self.closing = true;
        if !self.activated || self.zero {
            return Ok(());
        }
        if self.close_attached().is_ok() {
            return Ok(());
        }
        let evidence = self.recovery.as_ref().ok_or(WslError::CleanupUnconfirmed)?;
        WslGuestGuardian::close(&self.config.executable, evidence)
            .map_err(|_| WslError::CleanupUnconfirmed)?;
        self.zero = true;
        self.tunnel.confirm_zero();
        Ok(())
    }
    pub fn shutdown(&mut self) -> Result<(), WslError> {
        self.close_guest()?;
        self.tunnel.finish()
    }
}
