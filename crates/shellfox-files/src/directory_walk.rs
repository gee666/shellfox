//! One handle-relative no-follow walk shared by plans and ordinary file paths.
use crate::{paths, ErrorCode, FileError, HandleIdentity, Result};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use std::{
    ffi::OsString,
    path::{Component, Path},
};
pub(crate) struct DirectoryWalk {
    pub dirs: Vec<Dir>,
    pub identities: Vec<HandleIdentity>,
    pub names: Vec<OsString>,
    pub missing: Vec<OsString>,
}
pub(crate) fn walk(
    base: Dir,
    path: &Path,
    allow_missing: bool,
    retain_chain: bool,
    max_components: usize,
) -> Result<DirectoryWalk> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                if components.len() >= max_components {
                    return Err(FileError::new(
                        ErrorCode::Validation,
                        "Directory plan exceeds component bound.",
                    ));
                }
                components.push(name.to_owned());
            }
            Component::CurDir => {}
            _ => return Err(FileError::denied()),
        }
    }
    let metadata = base.dir_metadata()?;
    paths::reject_link(&metadata)?;
    let identity = HandleIdentity::from_directory(&base)?;
    let mut result = DirectoryWalk {
        dirs: vec![base],
        identities: vec![identity],
        names: vec![],
        missing: vec![],
    };
    for (index, name) in components.iter().enumerate() {
        let parent = result.dirs.last().ok_or_else(FileError::denied)?;
        let before = match parent.symlink_metadata(name) {
            Ok(info) => info,
            Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {
                result.missing = components[index..].to_vec();
                break;
            }
            Err(error) => return Err(error.into()),
        };
        // Path metadata is ONLY a type/reparse check, never identity evidence.
        paths::reject_link(&before)?;
        if !before.is_dir() {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Expected directory component.",
            ));
        }
        let child = parent.open_dir_nofollow(name)?;
        let after = child.dir_metadata()?;
        paths::reject_link(&after)?;
        let child_id = HandleIdentity::from_directory(&child)?;
        // Prove the no-follow edge against a SECOND relative held handle while
        // the first remains live. No projected Windows metadata comparison.
        let edge = parent.open_dir_nofollow(name)?;
        paths::reject_link(&edge.dir_metadata()?)?;
        if child_id != HandleIdentity::from_directory(&edge)? {
            return Err(FileError::conflict());
        }
        if retain_chain && result.identities.contains(&child_id) {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Cyclic directory ancestry cannot establish a topology proof.",
            ));
        }
        if retain_chain {
            result.names.push(name.clone());
        } else {
            result.dirs.clear();
            result.identities.clear();
            result.names.clear();
        }
        result.dirs.push(child);
        result.identities.push(child_id);
    }
    Ok(result)
}
