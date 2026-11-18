use anyhow::{ensure, Result};
use std::{
    process::Child,
    time::{Duration, Instant},
};

pub struct OwnedHost(pub Child);
impl OwnedHost {
    pub fn wait(&mut self, timeout: Duration) -> Result<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.0.try_wait()? {
                return Ok(status);
            }
            ensure!(
                Instant::now() < deadline,
                "Owned CLI host did not exit after typed Stop."
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for OwnedHost {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
