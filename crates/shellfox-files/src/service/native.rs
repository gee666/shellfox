//! Scopes over the app's own private directories, for metadata the host writes itself.
use super::*;
use crate::private::{self, Parent, PrivateDir, PrivateRead, PrivateVersion};
use cap_fs_ext::DirExt;
use std::ffi::OsStr;

/// A host-side condition checked before every write through a scope that carries it, such as
/// "this process still owns the data root". A failing check denies the write.
pub trait NativeCommitFence: Send + Sync {
    fn check(&self) -> Result<()>;
}

use crate::private::short_name;

/// The existing part of a path must be spelled exactly as the file system stores it, so two
/// configured roots cannot name one directory through case or short-name aliases.
fn not_aliased(existing: &Path) -> Result<()> {
    let actual = std::fs::canonicalize(existing)?;
    let actual = actual.to_str().ok_or_else(FileError::denied)?;
    let actual = actual.strip_prefix(r"\\?\").unwrap_or(actual);
    let expected = existing.to_str().ok_or_else(FileError::denied)?;
    let trim = |value: &str| {
        // Drive letters are case-insensitive and canonicalize upper-cases them.
        let value = value.trim_end_matches(['/', '\\']);
        let mut chars = value.chars();
        let first = chars.next().map(|c| c.to_ascii_uppercase());
        first.into_iter().chain(chars).collect::<String>()
    };
    if trim(actual) != trim(expected) {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Directory path is an alias of another spelling.",
        ));
    }
    Ok(())
}

fn unsupported() -> FileError {
    FileError::new(
        ErrorCode::Unsupported,
        "Private leaf operations need a private directory scope.",
    )
}

impl FileService {
    /// Read-write scope over a held private directory. Requests name leaves relative to that
    /// handle; the directory's path is never reopened. Trusted host code only.
    pub fn private_scope(
        directory: std::fs::File,
        fence: Arc<dyn NativeCommitFence>,
    ) -> Result<Arc<Self>> {
        fence.check()?;
        private::require_private_dir(&directory)?;
        Ok(Arc::new(Self {
            root: Some(Root {
                dir: Dir::from_std_file(directory),
                resolution: RootResolution::HandleOnly,
            }),
            kind: CapabilityKind::ApprovedRoot,
            access: Access::ReadWrite,
            revoked: Arc::new(AtomicBool::new(false)),
            inherited_revocations: Vec::new(),
            native_commit_fences: vec![fence],
        }))
    }

    /// Checks that `private_root(path)` would not pass through a link, junction or aliased
    /// name. Creates and changes nothing.
    pub fn check_private_root(path: &Path) -> Result<()> {
        let (root, relative) = paths::absolute_parts(path)?;
        let mut dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
        let mut existing = root;
        let names = relative
            .components()
            .map(|part| match part {
                std::path::Component::Normal(name) if !short_name(name) => Ok(name),
                _ => Err(FileError::denied()),
            })
            .collect::<Result<Vec<_>>>()?;
        for name in names {
            match dir.symlink_metadata(name) {
                Ok(info) => {
                    paths::reject_link(&info)?;
                    dir = dir.open_dir_nofollow(name)?;
                    existing.push(name);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            }
        }
        not_aliased(&existing)
    }

    /// Creates `path` and any missing ancestors as private app directories, or makes an
    /// existing final directory private. Ancestors that exist are only walked, never changed.
    /// Links, junctions and aliased names (case or 8.3 spelling) are refused before anything
    /// is created. The returned root holds the directory handle (pinned on Windows).
    pub fn private_root(path: &Path) -> Result<Arc<Self>> {
        let (root, relative) = paths::absolute_parts(path)?;
        let names: Vec<_> = relative
            .components()
            .map(|part| match part {
                std::path::Component::Normal(name) if !short_name(name) => Ok(name),
                _ => Err(FileError::denied()),
            })
            .collect::<Result<_>>()?;
        let (leaf, ancestors) = names.split_last().ok_or_else(FileError::denied)?;
        let mut dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
        let mut existing = root;
        let mut missing = false;
        for name in ancestors {
            if !missing {
                match dir.symlink_metadata(name) {
                    Ok(info) => {
                        paths::reject_link(&info)?;
                        dir = dir.open_dir_nofollow(name)?;
                        existing.push(name);
                        continue;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        not_aliased(&existing)?;
                        missing = true;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            let parent = dir.try_clone()?.into_std_file();
            let created = private::create_dir_with(&parent, name, Parent::App)?;
            dir = Dir::from_std_file(created.into_file());
        }
        let parent = dir.try_clone()?.into_std_file();
        let found = if missing {
            None
        } else {
            match dir.symlink_metadata(leaf) {
                Ok(info) => Some(info),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            }
        };
        let held: PrivateDir = match found {
            Some(info) => {
                paths::reject_link(&info)?;
                existing.push(leaf);
                not_aliased(&existing)?;
                let original = dir.open_dir_nofollow(leaf)?.into_std_file();
                private::repair_app_dir(&parent, leaf, &original)?;
                let held = private::open_app_dir(&parent, leaf)?;
                if held.identity() != &HandleIdentity::from_file(&original)? {
                    return Err(FileError::conflict());
                }
                held
            }
            None => {
                if !missing {
                    not_aliased(&existing)?;
                }
                private::create_dir_with(&parent, leaf, Parent::App)?
            }
        };
        Ok(Arc::new(Self {
            root: Some(Root {
                dir: Dir::from_std_file(held.into_file()),
                resolution: RootResolution::PathBound {
                    path: path.to_path_buf(),
                },
            }),
            kind: CapabilityKind::ApprovedRoot,
            access: Access::ReadWrite,
            revoked: Arc::new(AtomicBool::new(false)),
            inherited_revocations: Vec::new(),
            native_commit_fences: Vec::new(),
        }))
    }

    fn private_parent(&self) -> Result<std::fs::File> {
        if !self.is_private_scope() {
            return Err(unsupported());
        }
        let root = self.root.as_ref().ok_or_else(unsupported)?;
        Ok(root.dir.try_clone()?.into_std_file())
    }

    /// Opens or creates one private child directory as a narrower scope. It keeps this
    /// scope's fences and is revoked with it.
    pub fn private_child(self: &Arc<Self>, leaf: &OsStr) -> Result<Arc<Self>> {
        let parent = self.private_parent()?;
        // Under the lock, so a concurrent revoke() cannot be followed by a create.
        let _guard = Self::mutation_lock()?;
        self.can_derive_root()?;
        let child = match private::open_dir(&parent, leaf) {
            Err(error) if error.code == ErrorCode::NotFound => {
                self.authorized(true)?;
                private::create_dir(&parent, leaf)?
            }
            result => result?,
        };
        self.authorized(true)?;
        Ok(Arc::new(self.derive_bound_root(Root {
            dir: Dir::from_std_file(child.into_file()),
            resolution: RootResolution::HandleOnly,
        })?))
    }

    /// Reads one private leaf. Not found is `ErrorCode::NotFound`.
    pub fn read_private(&self, leaf: &OsStr, max: usize) -> Result<PrivateRead> {
        self.authorized(false)?;
        let read = private::read(&self.private_parent()?, leaf, max)?;
        self.authorized(false)?;
        Ok(read)
    }

    /// Replaces one private leaf if it still has the `expected` version (`None`: absent).
    /// Authorization and fences are checked again right before the rename.
    pub fn write_private(
        &self,
        leaf: &OsStr,
        bytes: &[u8],
        expected: Option<&PrivateVersion>,
    ) -> Result<PrivateVersion> {
        self.authorized(true)?;
        private::write_checked(&self.private_parent()?, leaf, bytes, expected, &|| {
            self.authorized(true)
        })
    }

    /// Removes a leaf this host wrote, if it is still exactly `expected`. The version is the
    /// authority here, so this also works after revocation, for shutdown cleanup.
    pub fn remove_private(&self, leaf: &OsStr, expected: &PrivateVersion) -> Result<()> {
        private::remove(&self.private_parent()?, leaf, expected)
    }
}

#[cfg(test)]
mod tests;
