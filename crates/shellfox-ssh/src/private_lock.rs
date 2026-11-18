use super::{Error, PrivateDirectory, Result};
use fs2::FileExt;
use std::{
    fs::File,
    sync::MutexGuard,
    thread,
    time::{Duration, Instant},
};
impl PrivateDirectory {
    pub(crate) fn lock(&self, name: &str) -> Result<StoreLock<'_>> {
        Self::name(name)?;
        // Like the retained host's save queue: local operations wait their turn
        // before starting the bounded cross-process contention deadline. Native ACL
        // checks remain mandatory but do not multiply same-host lock competition.
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
            match file.try_lock_exclusive() {
                Ok(()) => {
                    return Ok(StoreLock {
                        file,
                        _local: local,
                    })
                }
                Err(error)
                    if (error.kind() == std::io::ErrorKind::WouldBlock
                        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error())
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(20))
                }
                _ => return Err(Error::Busy),
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
        let _ = FileExt::unlock(&self.file);
    }
}
