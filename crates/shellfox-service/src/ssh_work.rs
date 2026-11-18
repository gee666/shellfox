use super::*;
use shellfox_ssh::{
    keys::{KeyStore, PreparedIdentity},
    system::SshLaunchRequest,
    HostKeyInfo, StoredProfile,
};
use std::{
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

pub(super) enum WorkEvent {
    Host {
        connection: Uuid,
        value: HostChallenge,
        reply: SyncSender<TrustDecision>,
    },
    Prompt {
        connection: Uuid,
        value: Prompt,
        reply: SyncSender<Option<Secret>>,
    },
    Prepared {
        connection: Uuid,
        result: Result<Prepared>,
    },
}
pub(super) struct Prepared {
    pub launch: Option<SshLaunchRequest>,
    pub identity: Option<PreparedIdentity>,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some(identity) = &self.identity {
            let _ = identity.cleanup();
        }
    }
}
#[derive(Clone)]
pub(super) struct Work {
    pub connection: Uuid,
    pub events: SyncSender<WorkEvent>,
    pub cancel: CancellationToken,
    pub lost: CancellationToken,
}
impl Work {
    pub fn check(&self) -> Result<()> {
        anyhow::ensure!(
            !self.cancel.is_cancelled() && !self.lost.is_cancelled(),
            "SSH connection cancelled."
        );
        Ok(())
    }
    fn wait<T>(&self, rx: Receiver<T>) -> Result<T> {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            self.check()?;
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(value) => {
                    self.check()?;
                    return Ok(value);
                }
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                _ => anyhow::bail!("SSH confirmation expired or closed."),
            }
        }
    }
    pub fn prompt(&self, label: &str, echo: bool) -> Result<Secret> {
        let (tx, rx) = mpsc::sync_channel(1);
        let label = label
            .chars()
            .take(1024)
            .map(|c| {
                if c <= '\u{1f}' || c == '\u{7f}' {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        self.events
            .try_send(WorkEvent::Prompt {
                connection: self.connection,
                value: Prompt {
                    prompt: Uuid::new_v4(),
                    label,
                    echo,
                },
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("SSH prompt queue unavailable."))?;
        self.wait(rx)?
            .ok_or_else(|| anyhow::anyhow!("SSH prompt cancelled."))
    }
    fn confirm(
        &self,
        host: &str,
        port: u16,
        key: &HostKeyInfo,
    ) -> shellfox_ssh::Result<TrustDecision> {
        self.check()
            .map_err(|_| shellfox_ssh::Error::HostNotTrusted)?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.events
            .try_send(WorkEvent::Host {
                connection: self.connection,
                value: HostChallenge {
                    challenge: Uuid::new_v4(),
                    host: host.into(),
                    port,
                    key_type: key.key_type.clone(),
                    fingerprint: key.fingerprint.clone(),
                },
                reply: tx,
            })
            .map_err(|_| shellfox_ssh::Error::HostNotTrusted)?;
        self.wait(rx)
            .map_err(|_| shellfox_ssh::Error::HostNotTrusted)
    }
    pub fn prepare(
        &self,
        backend: &SystemSshAdapter,
        directory: Arc<PrivateDirectory>,
        mut profile: StoredProfile,
        pty: RemotePty,
    ) -> Result<Prepared> {
        self.check()?;
        if profile.user.is_empty() {
            profile.user = self.prompt("Login name", true)?.expose().to_owned();
        }
        let mut launch = backend.prepare_launch_guarded(
            &profile,
            pty,
            [],
            |h, p, k| self.confirm(h, p, k),
            || self.check().map_err(|_| shellfox_ssh::Error::Permissions),
        )?;
        self.check()?;
        let identity = if let Some(file) = &profile.key_file {
            let bytes = directory.read_key_file(std::path::Path::new(file))?;
            let store = KeyStore::new(directory);
            let mut unlocked = store.prepare_bytes_for_system(&bytes, None, &mut rand::rngs::OsRng);
            let encrypted = matches!(unlocked, Err(shellfox_ssh::Error::PassphraseRequired));
            for _ in 0..if encrypted { 3 } else { 0 } {
                if !matches!(
                    unlocked,
                    Err(shellfox_ssh::Error::PassphraseRequired | shellfox_ssh::Error::InvalidKey)
                ) {
                    break;
                }
                let password = self.prompt("Private key passphrase", false)?;
                self.check()?;
                unlocked =
                    store.prepare_bytes_for_system(&bytes, Some(&password), &mut rand::rngs::OsRng);
            }
            let identity = unlocked?;
            let index = launch
                .args
                .iter()
                .position(|a| a == "-i")
                .ok_or_else(|| anyhow::anyhow!("SSH identity argument missing."))?;
            launch.args[index + 1] = identity.path().as_os_str().to_owned();
            // A server-controlled keyboard-interactive prompt must not impersonate
            // a local key-unlock prompt and obtain this encrypted identity's passphrase.
            launch.args.splice(
                0..0,
                [
                    "-o".into(),
                    "KbdInteractiveAuthentication=no".into(),
                    "-o".into(),
                    "PreferredAuthentications=publickey,password".into(),
                ],
            );
            Some(identity)
        } else {
            None
        };
        if let Err(error) = self.check() {
            if let Some(i) = &identity {
                let _ = i.cleanup();
            }
            return Err(error);
        }
        Ok(Prepared {
            launch: Some(launch),
            identity,
        })
    }
}
