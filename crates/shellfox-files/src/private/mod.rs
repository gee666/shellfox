//! Owner-only directories and files, always opened relative to a held parent handle.
//!
//! Creation is exclusive and private from the first moment: Windows passes the owner-only
//! DACL to `NtCreateFile`, Unix passes mode 0700/0600 to `mkdirat`/`openat`. Existing objects
//! are verified, and only `repair_app_dir` ever changes permissions.
use crate::{paths, ErrorCode, FileError, HandleIdentity, Result};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as native;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as native;
#[cfg(windows)]
mod windows_acl;
#[cfg(all(windows, test))]
pub(crate) use windows_acl::current_sid;

#[cfg(test)]
mod tests;

/// Upper bound for one private file read or write.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

/// Which parent-trust rule applies. App directories live under the user's profile, where
/// Windows grants extra principals rights on the parent; owning the parent is enough there.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Parent {
    Private,
    App,
}

/// A private directory opened relative to its parent. On Windows the handle denies delete
/// sharing, so the directory cannot be renamed or removed while it is held.
pub struct PrivateDir {
    parent: File,
    name: OsString,
    file: File,
    identity: HandleIdentity,
    policy: Parent,
}
impl PrivateDir {
    pub fn as_file(&self) -> &File {
        &self.file
    }
    pub fn identity(&self) -> &HandleIdentity {
        &self.identity
    }
    pub fn into_file(self) -> File {
        self.file
    }
    /// The name still opens this same directory, and it is still private.
    pub fn verify(&self) -> Result<()> {
        native::trusted_parent(&self.parent, self.policy)?;
        let named = native::open_dir(&self.parent, &self.name)?;
        if !self.identity.matches(&named)? || !self.identity.matches(&self.file)? {
            return Err(FileError::conflict());
        }
        native::require_private(&named, true)
    }
}

/// Content of a private file plus the version a later `write` or `remove` must match.
pub struct PrivateRead {
    pub bytes: Vec<u8>,
    pub version: PrivateVersion,
}
/// Object identity, size and SHA-256 of a private file's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateVersion {
    identity: HandleIdentity,
    size: u64,
    revision: String,
}
impl PrivateVersion {
    pub fn identity(&self) -> &HandleIdentity {
        &self.identity
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
}

pub(crate) fn leaf(name: &OsStr) -> Result<()> {
    let value = name.to_str().ok_or_else(FileError::denied)?;
    paths::validate(value)?;
    if value == "." || value == ".." || value.contains(['/', '\\', ':']) {
        return Err(FileError::denied());
    }
    Ok(())
}

fn finish_dir(parent: &File, name: &OsStr, file: File, policy: Parent) -> Result<PrivateDir> {
    native::require_private(&file, true)?;
    let dir = PrivateDir {
        parent: parent.try_clone()?,
        name: name.to_owned(),
        identity: HandleIdentity::from_file(&file)?,
        file,
        policy,
    };
    dir.verify()?;
    Ok(dir)
}

pub(crate) fn create_dir_with(parent: &File, name: &OsStr, policy: Parent) -> Result<PrivateDir> {
    leaf(name)?;
    native::trusted_parent(parent, policy)?;
    let file = native::create_dir(parent, name, policy == Parent::App)?;
    finish_dir(parent, name, file, policy)
}
pub(crate) fn open_dir_with(parent: &File, name: &OsStr, policy: Parent) -> Result<PrivateDir> {
    leaf(name)?;
    native::trusted_parent(parent, policy)?;
    let file = native::open_dir(parent, name)?;
    finish_dir(parent, name, file, policy)
}

/// Creates a new private directory. An existing entry is a conflict, never adopted.
pub fn create_dir(parent: &File, name: &OsStr) -> Result<PrivateDir> {
    create_dir_with(parent, name, Parent::Private)
}
/// Creates an application data directory. On Windows its DACL also grants SYSTEM.
pub fn create_app_dir(parent: &File, name: &OsStr) -> Result<PrivateDir> {
    create_dir_with(parent, name, Parent::App)
}
/// Opens an existing directory and verifies that it is private. Nothing is repaired.
pub fn open_dir(parent: &File, name: &OsStr) -> Result<PrivateDir> {
    open_dir_with(parent, name, Parent::Private)
}
/// Opens an existing application data directory under the app-parent rule.
pub fn open_app_dir(parent: &File, name: &OsStr) -> Result<PrivateDir> {
    open_dir_with(parent, name, Parent::App)
}

/// Makes the app's own existing data directory private. `original` is the caller's held
/// handle; the name must still open that same object. The parent is never changed.
pub fn repair_app_dir(parent: &File, name: &OsStr, original: &File) -> Result<()> {
    leaf(name)?;
    native::trusted_parent(parent, Parent::App)?;
    let identity = HandleIdentity::from_file(original)?;
    if !original.metadata()?.is_dir() {
        return Err(FileError::denied());
    }
    if native::require_private(original, true).is_ok() {
        return Ok(());
    }
    let file = native::open_dir_for_acl(parent, name)?;
    if !identity.matches(&file)? {
        return Err(FileError::conflict());
    }
    native::make_private(&file)?;
    native::require_private(&file, true)
}

fn version(file: &File, bytes: &[u8]) -> Result<PrivateVersion> {
    Ok(PrivateVersion {
        identity: HandleIdentity::from_file(file)?,
        size: bytes.len() as u64,
        revision: crate::content::revision(bytes),
    })
}

fn read_open(file: &mut File, max: usize) -> Result<PrivateRead> {
    if max == 0 || max > MAX_BYTES {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Invalid private file size limit.",
        ));
    }
    native::require_private(file, false)?;
    let identity = HandleIdentity::from_file(file)?;
    let size = file.metadata()?.len();
    if size > max as u64 {
        return Err(FileError::new(
            ErrorCode::Unsupported,
            "Private file exceeds its size limit.",
        ));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.seek(SeekFrom::Start(0))?;
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != size || !identity.matches(file)? {
        return Err(FileError::conflict());
    }
    Ok(PrivateRead {
        version: version(file, &bytes)?,
        bytes,
    })
}

/// Reads one private regular file with at most `max` bytes. A link, a hardlinked file or a
/// file readable by anyone else is refused.
pub fn read(parent: &File, name: &OsStr, max: usize) -> Result<PrivateRead> {
    leaf(name)?;
    native::trusted_parent(parent, Parent::Private)?;
    read_open(&mut native::open_file(parent, name)?, max)
}

fn current(parent: &File, name: &OsStr) -> Result<Option<PrivateVersion>> {
    match read(parent, name, MAX_BYTES) {
        Ok(read) => Ok(Some(read.version)),
        Err(error) if error.code == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes `bytes` to a new private temp file, flushes it, and renames it over `name`.
/// `expected` is the version the caller read: `None` means `name` must not exist yet.
/// Any other current state is a conflict and nothing is written.
pub fn write(
    parent: &File,
    name: &OsStr,
    bytes: &[u8],
    expected: Option<&PrivateVersion>,
) -> Result<PrivateVersion> {
    write_checked(parent, name, bytes, expected, &|| Ok(()))
}

/// `write`, with `check` run under the mutation lock right before the rename.
pub(crate) fn write_checked(
    parent: &File,
    name: &OsStr,
    bytes: &[u8],
    expected: Option<&PrivateVersion>,
    check: &dyn Fn() -> Result<()>,
) -> Result<PrivateVersion> {
    leaf(name)?;
    if bytes.len() > MAX_BYTES {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Private file exceeds its size limit.",
        ));
    }
    native::trusted_parent(parent, Parent::Private)?;
    let temp = OsString::from(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        uuid::Uuid::new_v4().simple()
    ));
    let mut stage = native::create_file(parent, &temp, true)?;
    let result = (|| {
        stage.write_all(bytes)?;
        stage.sync_all()?;
        let written = version(&stage, bytes)?;
        let _guard = crate::FileService::mutation_lock()?;
        if current(parent, name)?.as_ref() != expected {
            return Err(FileError::conflict());
        }
        check()?;
        native::rename(parent, &temp, &stage, name, expected.is_some())?;
        Ok(written)
    })();
    match result {
        Ok(written) => {
            // The new name is already visible; a failed directory flush is not a failed write.
            let _ = native::sync_dir(parent);
            Ok(written)
        }
        Err(error) => {
            let _ = native::remove(parent, &temp, &stage);
            Err(error)
        }
    }
}

/// Removes `name` if its content is still exactly `expected`.
pub fn remove(parent: &File, name: &OsStr, expected: &PrivateVersion) -> Result<()> {
    remove_checked(parent, name, expected, &|| Ok(()))
}

pub(crate) fn remove_checked(
    parent: &File,
    name: &OsStr,
    expected: &PrivateVersion,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    leaf(name)?;
    native::trusted_parent(parent, Parent::Private)?;
    let _guard = crate::FileService::mutation_lock()?;
    let mut file = native::open_file_for_delete(parent, name)?;
    if &read_open(&mut file, MAX_BYTES)?.version != expected {
        return Err(FileError::conflict());
    }
    check()?;
    native::remove(parent, name, &file)
}

#[cfg(test)]
pub(crate) fn make_test_private(file: &File) {
    native::make_private(file).unwrap();
}

pub(crate) fn require_private_dir(file: &File) -> Result<()> {
    native::require_private(file, true)
}

/// An empty owner-only temp file next to a user file, for staged saves. On Unix the parent
/// must not be writable by other users, so nobody else can swap the name before the rename.
pub(crate) fn create_stage(parent: &File, name: &OsStr) -> Result<File> {
    #[cfg(unix)]
    native::trusted_parent(parent, Parent::Private)?;
    native::create_file(parent, name, false)
}

/// Flushes a directory after a rename into it. Best effort: the new name is already visible,
/// so a failed flush does not turn a completed write into a reported failure.
pub(crate) fn sync_dir(dir: &cap_std::fs::Dir) {
    if let Ok(file) = dir.try_clone() {
        let _ = native::sync_dir(&file.into_std_file());
    }
}
