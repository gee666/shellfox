//! Explicit external identity selection. Parent handles are identity pins only,
//! never app-private directory authority. Key bytes come from the checked file.
use super::{platform, Error, PrivateDirectory, Result};
#[cfg(test)]
#[path = "private_identity_tests.rs"]
mod tests;
#[cfg(unix)]
use std::io::{Seek, SeekFrom};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};
use zeroize::Zeroizing;

#[cfg(windows)]
pub(super) fn native_helper_path(path: &Path) -> Result<PathBuf> {
    use std::path::Prefix;
    if !path.is_absolute() {
        return Err(Error::Validation);
    }
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) | Prefix::UNC(_, _) => {}
            _ => return Err(Error::Validation),
        },
        _ => return Err(Error::Validation),
    }
    for part in path.components() {
        match part {
            Component::ParentDir | Component::CurDir => return Err(Error::Validation),
            Component::Normal(name) => {
                let text = name.to_str().ok_or(Error::Validation)?;
                if text.ends_with(['.', ' '])
                    || text.contains(':')
                    || text.chars().any(char::is_control)
                {
                    return Err(Error::Validation);
                }
            }
            _ => {}
        }
    }
    Ok(path.components().map(|part| part.as_os_str()).collect())
}
struct ExternalIdentityParentPin {
    file: File,
    path: PathBuf,
}
struct CheckedIdentity {
    file: File,
    parent: ExternalIdentityParentPin,
    selection: PathBuf,
    observed: Revision,
}
#[derive(PartialEq, Eq)]
struct Revision {
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(unix)]
    mode: u32,
    #[cfg(windows)]
    attributes: u32,
}
fn revision(file: &File) -> Result<Revision> {
    let metadata = file.metadata()?;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(windows)]
    use std::os::windows::fs::MetadataExt;
    Ok(Revision {
        length: metadata.len(),
        modified: metadata.modified()?,
        #[cfg(unix)]
        changed: (metadata.ctime(), metadata.ctime_nsec()),
        #[cfg(unix)]
        mode: metadata.mode(),
        #[cfg(windows)]
        attributes: metadata.file_attributes(),
    })
}
impl CheckedIdentity {
    fn current(&self) -> Result<()> {
        platform::check(&self.file, false)?;
        if revision(&self.file)? != self.observed {
            return Err(Error::Permissions);
        }
        let parent = platform::identity_parent_handle(&self.parent.path)?;
        let file = platform::identity_file_handle(&self.selection)?;
        if !platform::same(&self.parent.file, &parent)? || !platform::same(&self.file, &file)? {
            return Err(Error::Permissions);
        }
        Ok(())
    }
}
impl PrivateDirectory {
    fn checked_identity(&self, path: &Path) -> Result<CheckedIdentity> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
            || path
                .to_str()
                .is_none_or(|text| text.contains(['%', '$']) || text.chars().any(char::is_control))
        {
            return Err(Error::Validation);
        }
        #[cfg(windows)]
        let selection = native_helper_path(path)?;
        #[cfg(not(windows))]
        let selection = path.to_owned();
        let parent_path = selection.parent().ok_or(Error::Validation)?.to_owned();
        let parent = ExternalIdentityParentPin {
            file: platform::identity_parent_handle(&parent_path)?,
            path: parent_path,
        };
        let file = platform::identity_file_handle(&selection)?;
        let observed = revision(&file)?;
        let checked = CheckedIdentity {
            file,
            parent,
            selection,
            observed,
        };
        checked.current()?;
        // External directory need not be private. The FILE owner/privacy remains
        // mandatory and is checked while the original parent/file are pinned.
        self.policy.permissions(&checked.selection, false, false)?;
        checked.current()?;
        Ok(checked)
    }
    pub(crate) fn check_identity(&self, path: &Path) -> Result<()> {
        self.checked_identity(path)?.current()
    }
    pub fn read_key_file(&self, path: &Path) -> Result<Zeroizing<Vec<u8>>> {
        self.check()?;
        let mut selected = self.checked_identity(path)?;
        let before = revision(&selected.file)?;
        if before.length > crate::keys::MAX_KEY_BYTES as u64 {
            return Err(Error::Limit);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        (&mut selected.file)
            .take(crate::keys::MAX_KEY_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > crate::keys::MAX_KEY_BYTES {
            return Err(Error::Limit);
        }
        if bytes.len() as u64 != before.length || revision(&selected.file)? != before {
            return Err(Error::Permissions);
        }
        #[cfg(unix)]
        {
            use sha2::{Digest, Sha256};
            // Unix read-only handles do not make contents immutable. Verify a
            // second bounded pass of this SAME descriptor before accepting it.
            let expected = Sha256::digest(&bytes[..]);
            selected.file.seek(SeekFrom::Start(0))?;
            let mut hash = Sha256::new();
            let mut total = 0usize;
            let mut buffer = [0u8; 16384];
            loop {
                let n = selected.file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                total = total.checked_add(n).ok_or(Error::Limit)?;
                if total > crate::keys::MAX_KEY_BYTES {
                    return Err(Error::Limit);
                }
                hash.update(&buffer[..n]);
            }
            if total != bytes.len()
                || hash.finalize() != expected
                || revision(&selected.file)? != before
            {
                return Err(Error::Permissions);
            }
        }
        selected.current()?;
        self.policy.permissions(&selected.selection, false, false)?;
        if revision(&selected.file)? != before {
            return Err(Error::Permissions);
        }
        selected.current()?;
        self.check()?;
        // No path reopen for bytes. The pins remain held through every check and
        // use of this read; the returned zeroizing value is the validated snapshot.
        Ok(bytes)
    }
}
