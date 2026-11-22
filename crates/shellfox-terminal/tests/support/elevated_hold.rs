use super::support::Fixture;
use std::{fs::File, os::windows::fs::OpenOptionsExt, sync::Arc};

/// A read-only, no-write-sharing handle on a synthetic `lazy.dll` beside the
/// fake helper. Tests use it to see when a session releases its extra hold.
pub fn lock(fixture: &Fixture) -> Arc<File> {
    let path = fixture.root.join("lazy.dll");
    std::fs::write(&path, b"owned synthetic asset").unwrap();
    const FILE_SHARE_READ: u32 = 1;
    Arc::new(
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
            .unwrap(),
    )
}
