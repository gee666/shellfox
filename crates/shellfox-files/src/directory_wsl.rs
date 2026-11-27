//! Directory prompts on Windows WSL shares, as 0.9 allowed: `\\wsl.localhost\<distro>\...`,
//! `\\wsl$\...` and the same with forward slashes. Only resolve, complete and create
//! reach a share. Other file operations keep refusing UNC paths: WSL tabs browse
//! guest files as their pinned Linux account instead.
//!
//! The WSL file server reports no file IDs, so the handle walk other roots use
//! cannot run here. As in 0.9 and the desktop prompt, these are path operations
//! that follow guest symlinks. They reveal folder names only, never file content.
use crate::{directory_ops::Candidates, paths, ErrorCode, FileError, FileService, Result};
use std::{fs, io, path::Path};

/// A share prompt path in its canonical backslash spelling, the form stored as a cwd.
pub(crate) struct SharePath {
    pub path: String,
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
        let path = share.windows_path();
        let root = format!(r"\\{}\{}", share.server, share.distro);
        let relative = path[root.len()..].trim_start_matches('\\');
        if !relative.is_empty() {
            paths::validate(relative)?;
        }
        if path.len() > 32767 {
            return Err(FileError::denied());
        }
        Ok(Self { path })
    }

    /// An existing directory. A missing one is NotFound only when its nearest
    /// existing ancestor is a directory, so the prompt may offer to create it.
    pub(crate) fn check(&self, files: &FileService) -> Result<()> {
        files.share_authorized()?;
        let path = Path::new(&self.path);
        match fs::metadata(path) {
            Ok(info) if info.is_dir() => Ok(()),
            Ok(_) => Err(not_directory()),
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            Err(error) => {
                for ancestor in path.ancestors().skip(1) {
                    match fs::metadata(ancestor) {
                        Ok(info) if info.is_dir() => break,
                        Ok(_) => return Err(not_directory()),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                Err(error.into())
            }
        }
    }

    pub(crate) fn complete(&self, files: &FileService, candidates: &mut Candidates) -> Result<()> {
        self.check(files)?;
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !candidates.wants(&name) {
                continue;
            }
            match fs::metadata(entry.path()) {
                Ok(info) if info.is_dir() => candidates.keep(name),
                Ok(_) => {}
                // A dangling or vanishing link is not a completion.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        files.share_authorized()
    }

    /// Recursive and idempotent below the nearest existing directory.
    pub(crate) fn create(&self, files: &FileService) -> Result<()> {
        match self.check(files) {
            Err(error) if error.code == ErrorCode::NotFound => {}
            other => return other,
        }
        let _guard = FileService::mutation_lock()?;
        files.authorized(true)?;
        fs::create_dir_all(&self.path)?;
        self.check(files)
    }
}

fn not_directory() -> FileError {
    FileError::new(
        ErrorCode::Validation,
        "Expected an accessible directory, not a file.",
    )
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
        let share = SharePath::parse("//wsl.localhost/Debian/home/./u/")
            .unwrap()
            .unwrap();
        assert_eq!(share.path, r"\\wsl.localhost\Debian\home\u");
        let root = SharePath::parse(r"\\wsl$\Debian").unwrap().unwrap();
        assert_eq!(root.path, r"\\wsl$\Debian");
        assert!(SharePath::parse(r"\\wsl$\Debian\a\..\b").unwrap().is_err());
        assert!(SharePath::parse(r"\\server\share").is_none());
    }

    /// Runs against the Debian distro when this host has one.
    #[test]
    fn prompts_resolve_complete_and_create_on_a_live_wsl_share() {
        use crate::{Access, ErrorCode, Location};
        if std::fs::read_dir(r"\\wsl.localhost\Debian\etc").is_err() {
            eprintln!("skipped: no Debian WSL share");
            return;
        }
        let files = FileService::local_desktop(Access::ReadWrite);
        let at = |path: &str| Location { path: path.into() };
        let resolve = |path: &str| files.resolve_directory(&at(path)).map(|r| r.cwd);
        let complete = |path: &str| files.complete_directory(&at(path)).unwrap().matches;
        for (input, cwd) in [
            ("//wsl.localhost/Debian/etc", r"\\wsl.localhost\Debian\etc"),
            (
                r"\\wsl.localhost\Debian\etc\",
                r"\\wsl.localhost\Debian\etc",
            ),
            (r#""\\wsl$\Debian\etc""#, r"\\wsl$\Debian\etc"),
            (r"\\wsl.localhost\Debian", r"\\wsl.localhost\Debian"),
        ] {
            assert_eq!(resolve(input).unwrap(), cwd, "{input}");
        }
        let missing = resolve(r"\\wsl$\Debian\shellfox-missing-dir").unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);
        let file = resolve(r"\\wsl$\Debian\etc\hostname\x").unwrap_err();
        assert_eq!(file.code, ErrorCode::Validation);

        for (input, expected) in [
            ("//wsl.localhost/Debian/et", "//wsl.localhost/Debian/etc/"),
            (r"\\wsl$\Debian\", r"\\wsl$\Debian\etc\"),
            (r"\\wsl.localhost\Debian", r"\\wsl.localhost\Debian\etc\"),
            (r"\\WSL.LOCALHOST\Debian\ET", r"\\WSL.LOCALHOST\Debian\etc\"),
        ] {
            let matches = complete(input);
            assert!(
                matches.iter().any(|m| m == expected),
                "{input}: {matches:?}"
            );
        }

        let root = format!(
            r"\\wsl.localhost\Debian\tmp\shellfox-prompt-{}",
            uuid::Uuid::new_v4()
        );
        let nested = format!("{root}/a/b");
        let created = files
            .create_directory_with_context(&at(&nested), None)
            .unwrap();
        assert_eq!(created.cwd, format!(r"{root}\a\b"));
        std::fs::remove_dir_all(&root).unwrap();

        let approved = FileService::approved_root(std::env::temp_dir(), Access::ReadOnly).unwrap();
        let denied = approved
            .resolve_directory(&at(r"\\wsl$\Debian\etc"))
            .unwrap_err();
        assert_eq!(denied.code, ErrorCode::AccessDenied);
    }
}
