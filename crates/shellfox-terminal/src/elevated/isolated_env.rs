use super::AdapterError;
use crate::EnvironmentPolicy;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) fn local_directory(path: &Path) -> Result<PathBuf, AdapterError> {
    use std::os::windows::fs::MetadataExt;
    let path = crate::environment::lookup_path(path.to_path_buf());
    let text = path.to_str().ok_or(AdapterError::InvalidLaunch)?;
    let bytes = text.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
        || text[2..]
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '"' | '*' | '?' | '<' | '>' | '|'))
    {
        return Err(AdapterError::InvalidLaunch);
    }
    if text[3..]
        .split(['/', '\\'])
        .any(|part| part == "." || part == ".." || part.ends_with('.') || part.ends_with(' '))
    {
        return Err(AdapterError::InvalidLaunch);
    }
    for parent in path.ancestors() {
        let metadata =
            std::fs::symlink_metadata(parent).map_err(|_| AdapterError::InvalidLaunch)?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(AdapterError::InvalidLaunch);
        }
    }
    Ok(crate::environment::lookup_path(
        std::fs::canonicalize(path).map_err(|_| AdapterError::InvalidLaunch)?,
    ))
}

pub(super) fn environment(
    root: &Path,
    policy: EnvironmentPolicy,
) -> Result<BTreeMap<String, String>, AdapterError> {
    let built = crate::environment_paths::build(root, policy, &[])
        .map_err(|_| AdapterError::InvalidLaunch)?;
    let mut env = BTreeMap::new();
    for (key, value) in built {
        env.insert(
            key.into_string().map_err(|_| AdapterError::InvalidLaunch)?,
            value
                .into_string()
                .map_err(|_| AdapterError::InvalidLaunch)?,
        );
    }
    // The additive helper protocol is Windows PowerShell only. Do not pass the
    // local experiment's additional pwsh module installation through this mode.
    let system = crate::windows::system_directory().map_err(|_| AdapterError::InvalidLaunch)?;
    env.insert(
        "PSModulePath".into(),
        system
            .join("WindowsPowerShell/v1.0/Modules")
            .into_os_string()
            .into_string()
            .map_err(|_| AdapterError::InvalidLaunch)?,
    );
    let search = env.get("PATH").ok_or(AdapterError::InvalidLaunch)?;
    let directories: Vec<_> = std::env::split_paths(search)
        .filter_map(|path| local_directory(&path).ok())
        .collect();
    if directories.is_empty() {
        return Err(AdapterError::InvalidLaunch);
    }
    env.insert(
        "PATH".into(),
        std::env::join_paths(directories)
            .map_err(|_| AdapterError::InvalidLaunch)?
            .into_string()
            .map_err(|_| AdapterError::InvalidLaunch)?,
    );
    Ok(env)
}
