use super::{input::Input, owner::Owner};
use crate::raw_process::io::Output;
use anyhow::{ensure, Context, Result};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::Child,
    thread::JoinHandle,
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
pub(super) struct GuestExit {
    pub code: i32,
    pub signal9: bool,
}
pub(super) struct Inner {
    pub child: Child,
    pub owner: Owner,
    pub assigned: bool,
    pub control: Option<UnixStream>,
    pub input: Option<Input>,
    pub output: Output,
    pub readers: Vec<JoinHandle<()>>,
    pub guest: Option<GuestExit>,
    root: bool,
    frame: Vec<u8>,
    stop_sent: bool,
    removed: bool,
}
impl Inner {
    pub fn new(child: Child, owner: Owner) -> Self {
        Self {
            child,
            owner,
            assigned: false,
            control: None,
            input: None,
            output: Output::default(),
            readers: vec![],
            guest: None,
            root: false,
            frame: Vec::new(),
            stop_sent: false,
            removed: false,
        }
    }
    /// The guest's status is reported or its controller is gone.
    pub fn settled(&self) -> bool {
        self.guest.is_some() || self.root
    }
    pub fn attach(&mut self) -> Result<()> {
        self.owner.attach(&self.child)?;
        self.assigned = true;
        Ok(())
    }
    fn read_report(&mut self) -> Result<()> {
        if self.guest.is_none() {
            if let Some(control) = &mut self.control {
                let mut bytes = [0; 5];
                loop {
                    match control.read(&mut bytes[..5 - self.frame.len()]) {
                        Ok(0) => break,
                        Ok(count) => {
                            self.frame.extend_from_slice(&bytes[..count]);
                            if self.frame.len() == 5 {
                                let code = i32::from_le_bytes(self.frame[..4].try_into()?);
                                ensure!(
                                    (0..=255).contains(&code)
                                        && self.frame[4] <= 1
                                        && (self.frame[4] == 0 || code == 137),
                                    "invalid guest exit report"
                                );
                                self.guest = Some(GuestExit {
                                    code,
                                    signal9: self.frame[4] == 1,
                                });
                                break;
                            }
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                            ) =>
                        {
                            break
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        Ok(())
    }
    fn observe(&mut self) -> Result<()> {
        self.read_report()?;
        if !self.root {
            self.root = self.owner.exited(&mut self.child)?;
        }
        // A report may arrive between the first nonblocking read and waitpid.
        // Drain again after original-root exit before declaring status unknown.
        if self.root {
            self.read_report()?;
        }
        Ok(())
    }
    pub fn poll(&mut self) -> Result<Option<i32>> {
        self.observe()?;
        if self.guest.is_some() || self.root {
            if let Some(input) = &self.input {
                input.close();
            }
            if self.assigned {
                self.owner.terminate()?;
            }
            if self.root && self.owner.empty()? {
                // Controller SIGKILL is cleanup, never the guest's exit code.
                return self
                    .guest
                    .map(|guest| Some(guest.code))
                    .context("raw guest exit status unconfirmed; original owner retained");
            }
        }
        Ok(None)
    }
    pub fn stop(&mut self) -> Result<bool> {
        self.observe()?;
        if self.guest.is_some() || self.root || self.stop_sent {
            return Ok(false);
        }
        if let Some(control) = &mut self.control {
            control
                .write_all(b"K")
                .context("request original raw guest SIGKILL")?;
            self.stop_sent = true;
            return Ok(true);
        }
        Ok(false)
    }
    pub fn close(&mut self) -> Result<()> {
        if self.removed {
            return Ok(());
        }
        self.output.discard();
        if let Some(input) = &self.input {
            input.close();
        }
        if self.assigned && self.control.is_some() {
            if self.stop().is_err() {
                self.owner.terminate()?;
            }
        } else if self.assigned {
            self.owner.terminate()?;
        } else {
            self.owner.stop_unreleased(&mut self.child)?;
        }
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            self.observe()?;
            if self.assigned && (self.guest.is_some() || self.root) {
                self.owner.terminate()?;
            }
            let input_done = self.input.as_mut().map_or(Ok(true), Input::join)?;
            if self.root
                && self.owner.empty()?
                && input_done
                && self.readers.iter().all(JoinHandle::is_finished)
            {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "raw cleanup unconfirmed; original owners retained"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        for reader in self.readers.drain(..) {
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("raw output worker panicked"))?;
        }
        self.owner.remove(&mut self.child)?;
        self.removed = true;
        Ok(())
    }
}
