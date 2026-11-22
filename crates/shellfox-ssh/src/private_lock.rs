use super::{Error, PrivateDirectory, Result};
use std::{
    fs::{File, TryLockError},
    sync::MutexGuard,
    thread,
    time::{Duration, Instant},
};
impl PrivateDirectory {
    pub(crate) fn lock(&self, name: &str) -> Result<StoreLock<'_>> {
        Self::name(name)?;
        // Local operations queue on the mutex first, so the bounded cross-process
        // wait below only covers contention with another process.
        let local = self.serial.lock().map_err(|_| Error::Storage)?;
        let file = match self.open_file(name, true) {
            Ok(file) => file,
            Err(Error::Storage) if self.dir.symlink_metadata(name).is_ok() => {
                self.open_file(name, false)?
            }
            Err(error) => return Err(error),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match file.try_lock() {
                Ok(()) => {
                    return Ok(StoreLock {
                        file,
                        _local: local,
                    })
                }
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20))
                }
                Err(_) => return Err(Error::Busy),
            }
        }
    }
}
// Persistent inode: never unlink a file-lock name and create two independent locks.
pub(crate) struct StoreLock<'a> {
    file: File,
    _local: MutexGuard<'a, ()>,
}
impl Drop for StoreLock<'_> {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
