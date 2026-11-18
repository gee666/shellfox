//! Sync the original directory through a readable descriptor, not cap-std's O_PATH lookup FD.
use crate::{FileError, HandleIdentity, Result};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, OpenOptions};
pub(crate) fn sync_directory(directory: &Dir) -> Result<()> {
    let identity = HandleIdentity::from_directory(directory)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No);
    // Literal '.' is relative to the ACTUAL held directory. No label/path is resolved.
    let readable = directory.open_with(".", &options)?;
    if !readable.metadata()?.is_dir() || HandleIdentity::from_cap_file(&readable)? != identity {
        return Err(FileError::conflict());
    }
    readable.sync_all()?;
    if HandleIdentity::from_directory(directory)? != identity {
        return Err(FileError::conflict());
    }
    Ok(())
}
