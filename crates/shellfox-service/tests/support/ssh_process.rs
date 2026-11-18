//! Test-only self-executable, no descendants, accounts, SSH sockets or user config.
use anyhow::Result;
use std::{
    collections::BTreeMap,
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
pub struct FakeProcess {
    child: Child,
    incoming: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
    open: Arc<AtomicBool>,
}
impl FakeProcess {
    pub fn spawn(
        command: &std::ffi::OsStr,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .args(["--exact", "fixture_ssh_process", "--nocapture"])
            .envs(environment)
            .env("SHELLFOX_SSH_FIXTURE_COMMAND", command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("No fixture stdout"))?;
        let (tx, incoming) = mpsc::sync_channel(16);
        let open = Arc::new(AtomicBool::new(true));
        let flag = open.clone();
        let reader = match std::thread::Builder::new()
            .name("fake-ssh-output".into())
            .spawn(move || {
                let mut stdout = stdout.take(65536);
                let mut buffer = [0u8; 4096];
                while let Ok(length) = stdout.read(&mut buffer) {
                    if length == 0 || tx.try_send(buffer[..length].to_vec()).is_err() {
                        break;
                    }
                }
                flag.store(false, Ordering::SeqCst);
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        };
        Ok(Self {
            child,
            incoming,
            reader: Some(reader),
            open,
        })
    }
    pub fn read(&self, remaining: Duration) -> Result<Vec<u8>> {
        self.incoming
            .recv_timeout(remaining)
            .map_err(|_| anyhow::anyhow!("Fixture output deadline"))
    }
    pub fn output_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }
    pub fn is_running(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_none())
    }
    pub fn close(&mut self) -> Result<()> {
        drop(self.child.stdin.take());
        let until = Instant::now() + Duration::from_secs(2);
        while self.child.try_wait()?.is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.child.try_wait()?.is_none() {
            self.child.kill()?
        }
        self.child.wait()?;
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("Fixture reader failed"))?;
        }
        Ok(())
    }
}
impl Drop for FakeProcess {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
