//! Directory prompts on Windows WSL shares, as 0.9 allowed: `\\wsl.localhost\<distro>\...`,
//! `\\wsl$\...` and the same with forward slashes. Only resolve, complete and create
//! reach a share. Other file operations keep refusing UNC paths: WSL tabs browse
//! guest files as their pinned Linux account instead.
use crate::{paths, FileError, FileService, Result};
use cap_std::{ambient_authority, fs::Dir};
use std::path::PathBuf;

/// A share prompt path split into its root and the components under it.
pub(crate) struct SharePath {
    /// Canonical backslash spelling, the form stored as a session cwd.
    pub path: String,
    root: String,
    relative: PathBuf,
}

impl SharePath {
    /// None when `value` is not a WSL share or this host is not Windows.
    pub(crate) fn parse(value: &str) -> Option<Result<Self>> {
        if !cfg!(windows) {
            return None;
        }
        let share = shellfox_core::wsl_share(value)?;
        Some(Self::checked(share))
    }

    fn checked(share: shellfox_core::WslShare<'_>) -> Result<Self> {
        let root = format!(r"\\{}\{}\", share.server, share.distro);
        let path = share.windows_path();
        let relative = path[root.len().min(path.len())..].to_owned();
        if !relative.is_empty() {
            paths::validate(&relative)?;
        }
        if path.len() > 32767 {
            return Err(FileError::denied());
        }
        Ok(Self {
            path,
            root,
            relative: relative.split('\\').filter(|p| !p.is_empty()).collect(),
        })
    }

    /// The same share with the last component dropped: its parent, or itself at the root.
    pub(crate) fn parent(&self) -> Option<Self> {
        let relative = self.relative.parent()?.to_path_buf();
        let mut path = self.root.trim_end_matches('\\').to_owned();
        for part in relative.iter() {
            path.push('\\');
            path.push_str(part.to_str()?);
        }
        Some(Self {
            path,
            root: self.root.clone(),
            relative,
        })
    }

    /// The share's directory, walked without following links like any other root.
    pub(crate) fn open(&self, files: &FileService) -> Result<Dir> {
        files.share_authorized()?;
        let root = Dir::open_ambient_dir(&self.root, ambient_authority())?;
        paths::descend(root, &self.relative)
    }

    /// Recursive, idempotent creation under the share root.
    pub(crate) fn create(&self, files: &FileService) -> Result<()> {
        files.share_authorized()?;
        let _guard = FileService::mutation_lock()?;
        files.authorized(true)?;
        let root = Dir::open_ambient_dir(&self.root, ambient_authority())?;
        paths::create_all(root, &self.relative)
    }
}

impl FileService {
    /// Shares are account paths: an approved or private root never reaches one.
    fn share_authorized(&self) -> Result<()> {
        self.require_path_resolution()?;
        if self.has_root() {
            return Err(FileError::denied());
        }
        self.authorized(false)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn share_prompts_keep_one_canonical_spelling_and_refuse_traversal() {
        let share = SharePath::parse("//wsl.localhost/Debian/home/./u/").unwrap().unwrap();
        assert_eq!(share.path, r"\\wsl.localhost\Debian\home\u");
        assert_eq!(share.parent().unwrap().path, r"\\wsl.localhost\Debian\home");
        let root = SharePath::parse(r"\\wsl$\Debian").unwrap().unwrap();
        assert_eq!(root.path, r"\\wsl$\Debian");
        assert!(root.parent().is_none());
        assert!(SharePath::parse(r"\\wsl$\Debian\a\..\b").unwrap().is_err());
        assert!(SharePath::parse(r"\\server\share").is_none());
    }
}
