use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub struct Release(pub PathBuf);
impl Release {
    pub fn now(&self) {
        std::fs::write(self.0.join("release"), b"exit original fixture").unwrap();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release"), b"fixture cleanup");
    }
}
pub async fn wait_file(path: &Path) {
    let end = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < end,
            "fixture did not reach its controlled phase"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
pub fn lease_held(directory: &Path) -> bool {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    match options.open(directory.join("process-lease")) {
        Err(_) => true,
        Ok(file) => {
            #[cfg(unix)]
            {
                rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
                    .is_err()
            }
            #[cfg(not(unix))]
            {
                let _ = file;
                false
            }
        }
    }
}
pub async fn wait_released(directory: &Path) {
    let end = Instant::now() + Duration::from_secs(5);
    while lease_held(directory) {
        assert!(
            Instant::now() < end,
            "original fixture process did not exit"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
