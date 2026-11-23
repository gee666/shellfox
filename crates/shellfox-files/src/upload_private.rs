use crate::paths;
use crate::{FileError, Result};
#[cfg(test)]
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::File;
#[cfg(test)]
use cap_std::fs::OpenOptions;

/// Owned fixture setup only. Runtime journal admission verifies and never repairs existing roots.
#[cfg(test)]
pub(crate) fn protect_retained_directory(directory: &cap_std::fs::Dir) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No);
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.access_mode(0x1 | 0x80 | 0x20000 | 0x40000 | 0x100000);
    }
    let file = directory.open_with(".", &options)?;
    let info = file.metadata()?;
    paths::reject_link(&info)?;
    if !info.is_dir()
        || !crate::HandleIdentity::from_file(&directory.try_clone()?.into_std_file())?
            .matches(&file.try_clone()?.into_std())?
    {
        return Err(FileError::denied());
    }
    protect_handle(&file, true)?;
    Ok(file)
}

pub(crate) fn require_owner(file: &File, directory: &cap_std::fs::Dir) -> Result<()> {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        if file.metadata()?.uid() != directory.dir_metadata()?.uid() {
            return Err(FileError::denied());
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        crate::permissions::same_owner(
            file,
            &File::from_std(directory.try_clone()?.into_std_file()),
        )
    }
}

pub(crate) fn protect_file(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    paths::reject_link(&metadata)?;
    if !metadata.is_file() {
        return Err(FileError::denied());
    }
    protect_handle(file, false)
}

#[cfg(unix)]
fn protect_handle(file: &File, directory: bool) -> Result<()> {
    use cap_std::fs::{MetadataExt, PermissionsExt};
    // SAFETY: geteuid has no arguments or ownership requirements.
    if file.metadata()?.uid() != unsafe { libc::geteuid() } {
        return Err(FileError::denied());
    }
    file.set_permissions(cap_std::fs::Permissions::from_mode(if directory {
        0o700
    } else {
        0o600
    }))?;
    Ok(())
}

#[cfg(windows)]
fn protect_handle(file: &File, directory: bool) -> Result<()> {
    let descriptor = crate::private_security::Security::new(directory)?;
    descriptor.apply(&file.try_clone()?.into_std())
}
