//! Publication stays relative to the retained native directory on every platform.
use crate::{paths::Target, Result};
use cap_std::fs::{Dir, File};
use std::path::Path;

pub(crate) fn replace(
    directory: &Dir,
    staged: &Path,
    destination: &Path,
    expected: &File,
) -> Result<()> {
    let source = Target {
        parent: directory.try_clone()?,
        name: staged.to_path_buf(),
        absolute: staged.to_path_buf(),
    };
    let target = Target {
        parent: directory.try_clone()?,
        name: destination.to_path_buf(),
        absolute: destination.to_path_buf(),
    };
    crate::platform::rename_replace(&source, &target, expected)
}
