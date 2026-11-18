//! Checks preexisting experiment state paths without reading their contents.
use anyhow::{ensure, Context, Result};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsolationPathKind {
    Directory,
    File,
}

/// Reject existing redirects, shared file aliases and paths outside the root.
/// A missing final component is allowed below an existing in-root parent. This is
/// not a defense against a hostile process racing under the same account.
pub fn validate_isolated_path(path: &Path, root: &Path, kind: IsolationPathKind) -> Result<()> {
    ensure!(
        path.is_absolute() && root.is_absolute(),
        "isolated paths must be absolute"
    );
    let canonical_root = std::fs::canonicalize(root).context("resolve isolation root")?;
    ensure!(
        canonical_root.is_dir(),
        "isolation root must be a directory"
    );
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().context("isolated path has no parent")?;
            ensure!(
                std::fs::canonicalize(parent)?.starts_with(&canonical_root),
                "isolated path escapes experiment root: {}",
                path.display()
            );
            return Ok(());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("inspect isolated path {}", path.display()))
        }
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "isolated path must not be a symlink or reparse point: {}",
            path.display()
        );
    }
    ensure!(
        !metadata.file_type().is_symlink(),
        "isolated path must not be a symlink or reparse point: {}",
        path.display()
    );
    ensure!(
        match kind {
            IsolationPathKind::Directory => metadata.is_dir(),
            IsolationPathKind::File => metadata.is_file(),
        },
        "isolated path has an unexpected file type: {}",
        path.display()
    );
    if kind == IsolationPathKind::File {
        #[cfg(windows)]
        crate::windows::validate_single_link_file(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.nlink() == 1,
                "isolated file must not have hardlink aliases: {}",
                path.display()
            );
        }
    }
    ensure!(
        std::fs::canonicalize(path)?.starts_with(&canonical_root),
        "isolated path escapes experiment root: {}",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preexisting_hardlink_alias_cannot_share_state_with_another_root() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../tmp/terminal-links-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(scratch.join("private")).unwrap();
        std::fs::create_dir(scratch.join("outside")).unwrap();
        let scratch = std::path::absolute(scratch).unwrap();
        let outside = scratch.join("outside/owned-fixture");
        let private = scratch.join("private/config");
        std::fs::write(&outside, "owned fixture, no credentials").unwrap();
        assert!(validate_isolated_path(
            &scratch.join("outside/missing"),
            &scratch.join("private"),
            IsolationPathKind::File
        )
        .is_err());
        validate_isolated_path(&private, &scratch.join("private"), IsolationPathKind::File)
            .unwrap();
        std::fs::hard_link(&outside, &private).unwrap();
        let error =
            validate_isolated_path(&private, &scratch.join("private"), IsolationPathKind::File)
                .unwrap_err();
        assert!(
            format!("{error:#}").contains("hardlink aliases"),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(outside).unwrap(),
            "owned fixture, no credentials"
        );
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
