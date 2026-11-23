use super::{error, SystemResult};
use shellfox_protocol::ErrorCode;
use std::{
    fs::{self, File, OpenOptions, Permissions},
    io::{Read, Write},
    path::{Component, Path},
    time::UNIX_EPOCH,
};

pub const MAX_SYSTEM_FILE: u64 = 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRevision {
    pub device: u64,
    pub id: u64,
    pub length: u64,
    pub modified: u128,
    pub attributes: u32,
    pub changed: i128,
}
#[derive(Clone, Debug)]
pub struct SystemFile {
    pub bytes: Vec<u8>,
    pub revision: FileRevision,
    pub permissions: Permissions,
    pub executable: bool,
}
/// Replacements compare the observed identity/revision, stage new bytes, preserve
/// permissions and recheck before commit. No query may create a missing path.
pub trait SystemFiles: Send + Sync {
    fn read(&self, path: &Path) -> SystemResult<Option<SystemFile>>;
    fn directory(&self, path: &Path) -> SystemResult<bool>;
    fn program(&self, path: &Path) -> SystemResult<bool>;
    fn canonical_directory(&self, path: &Path) -> SystemResult<std::path::PathBuf>;
    fn replace(
        &self,
        path: &Path,
        expected: Option<&SystemFile>,
        bytes: Option<&[u8]>,
        executable: bool,
    ) -> SystemResult<()>;
}
#[derive(Default)]
pub struct NativeSystemFiles;
impl SystemFiles for NativeSystemFiles {
    fn read(&self, path: &Path) -> SystemResult<Option<SystemFile>> {
        super::validation::absolute_path(path)?;
        safe_ancestors(path)?;
        let before = match fs::symlink_metadata(path) {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(storage("System integration file cannot be inspected.")),
        };
        reject_alias(&before)?;
        if before.len() > MAX_SYSTEM_FILE {
            return Err(storage("System integration file exceeds 1 MiB."));
        }
        let file =
            File::open(path).map_err(|_| storage("System integration file cannot be read."))?;
        let revision = file_revision(&file)?;
        let permissions = file
            .metadata()
            .map_err(|_| storage("System file metadata unavailable."))?
            .permissions();
        let mut bytes = Vec::new();
        (&file)
            .take(MAX_SYSTEM_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| storage("System integration file read failed."))?;
        if bytes.len() as u64 > MAX_SYSTEM_FILE || file_revision(&file)? != revision {
            return Err(conflict());
        }
        safe_ancestors(path)?;
        reject_alias(&fs::symlink_metadata(path).map_err(|_| conflict())?)?;
        let current = File::open(path).map_err(|_| conflict())?;
        if file_revision(&current)? != revision {
            return Err(conflict());
        }
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            permissions.mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = true;
        Ok(Some(SystemFile {
            bytes,
            revision,
            permissions,
            executable,
        }))
    }
    fn directory(&self, path: &Path) -> SystemResult<bool> {
        super::validation::absolute_path(path)?;
        match fs::metadata(path) {
            Ok(value) => Ok(value.is_dir()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(storage("System directory is inaccessible.")),
        }
    }
    fn program(&self, path: &Path) -> SystemResult<bool> {
        super::validation::absolute_path(path)?;
        match fs::metadata(path) {
            Ok(value) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    Ok(value.is_file() && value.permissions().mode() & 0o111 != 0)
                }
                #[cfg(not(unix))]
                {
                    Ok(value.is_file())
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(storage("System program cannot be inspected.")),
        }
    }
    fn canonical_directory(&self, path: &Path) -> SystemResult<std::path::PathBuf> {
        super::validation::absolute_path(path)?;
        if !self.directory(path)? {
            return Err(error(ErrorCode::NotFound, "Directory does not exist."));
        }
        let value = fs::canonicalize(path).map_err(|_| storage("Directory cannot be resolved."))?;
        shellfox_transport::normalize_private_path(&value)
            .map_err(|_| error(ErrorCode::Validation, "Unsupported directory path."))
    }
    fn replace(
        &self,
        path: &Path,
        expected: Option<&SystemFile>,
        bytes: Option<&[u8]>,
        executable: bool,
    ) -> SystemResult<()> {
        super::validation::absolute_path(path)?;
        safe_ancestors(path)?;
        compare(self.read(path)?.as_ref(), expected)?;
        super::file_platform::validate_mutation(path, expected.map(|file| &file.revision))?;
        let Some(bytes) = bytes else {
            if expected.is_some() {
                compare(self.read(path)?.as_ref(), expected)?;
                fs::remove_file(path)
                    .map_err(|_| storage("Owned integration file could not be removed."))?;
            }
            return Ok(());
        };
        if bytes.len() as u64 > MAX_SYSTEM_FILE {
            return Err(storage("System integration output exceeds 1 MiB."));
        }
        let parent = path
            .parent()
            .ok_or_else(|| storage("Integration parent unavailable."))?;
        fs::create_dir_all(parent)
            .map_err(|_| storage("Integration directory cannot be created."))?;
        safe_ancestors(path)?;
        let staged = parent.join(format!(".shellfox-{}.new", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(if executable { 0o755 } else { 0o644 });
            }
            let mut file = options
                .open(&staged)
                .map_err(|_| storage("Integration staging file cannot be created."))?;
            file.write_all(bytes)
                .map_err(|_| storage("Integration staging write failed."))?;
            if let Some(old) = expected {
                super::file_platform::prepare_permissions(&file, path, &old.revision, executable)?;
            }
            file.sync_all()
                .map_err(|_| storage("Integration staging flush failed."))?;
            drop(file);
            compare(self.read(path)?.as_ref(), expected)?;
            if expected.is_none() {
                fs::hard_link(&staged, path).map_err(|_| conflict())?;
                fs::remove_file(&staged)
                    .map_err(|_| storage("Integration staging cleanup failed."))?;
            } else {
                super::file_platform::commit_existing(path, &staged)?;
            }
            Ok(())
        })();
        let _ = fs::remove_file(&staged);
        result
    }
}
fn compare(current: Option<&SystemFile>, expected: Option<&SystemFile>) -> SystemResult<()> {
    if current.map(|file| &file.revision) != expected.map(|file| &file.revision)
        || current.map(|file| &file.bytes) != expected.map(|file| &file.bytes)
    {
        return Err(conflict());
    }
    Ok(())
}
fn reject_alias(info: &fs::Metadata) -> SystemResult<()> {
    if !info.is_file() || info.file_type().is_symlink() {
        return Err(error(
            ErrorCode::AuthFailed,
            "Integration path is not an owned regular file.",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0 {
            return Err(error(
                ErrorCode::AuthFailed,
                "Integration reparse point refused.",
            ));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if info.nlink() != 1 {
            return Err(error(
                ErrorCode::AuthFailed,
                "Integration hardlink alias refused.",
            ));
        }
    }
    Ok(())
}
fn safe_ancestors(path: &Path) -> SystemResult<()> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        current.push(component);
        if !matches!(component, Component::Normal(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(info) => {
                if info.file_type().is_symlink() {
                    return Err(error(
                        ErrorCode::AuthFailed,
                        "Integration symbolic-link ancestor refused.",
                    ));
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if info.file_attributes() & 0x400 != 0 {
                        return Err(error(
                            ErrorCode::AuthFailed,
                            "Integration reparse ancestor refused.",
                        ));
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(storage("Integration ancestor cannot be inspected.")),
        }
    }
    Ok(())
}
pub(crate) fn file_revision(file: &File) -> SystemResult<FileRevision> {
    let info = file
        .metadata()
        .map_err(|_| storage("System file identity unavailable."))?;
    reject_alias(&info)?;
    let (device, id, attributes, changed) = super::file_platform::identity(file)?;
    let modified = info
        .modified()
        .map_err(|_| storage("System file revision unavailable."))?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| storage("Invalid system file revision."))?
        .as_nanos();
    Ok(FileRevision {
        device,
        id,
        length: info.len(),
        modified,
        attributes,
        changed,
    })
}
fn storage(message: &str) -> shellfox_protocol::AppError {
    error(ErrorCode::StorageFailed, message)
}
fn conflict() -> shellfox_protocol::AppError {
    error(
        ErrorCode::FileConflict,
        "Integration file changed since inspection; no automatic retry.",
    )
}
