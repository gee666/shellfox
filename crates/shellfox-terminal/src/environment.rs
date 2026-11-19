//! Executable discovery and writable state are deliberately separate.
#[cfg(any(windows, target_os = "linux", test))]
use anyhow::{ensure, Result};
#[cfg(any(windows, target_os = "linux", test))]
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnvironmentPolicy {
    #[default]
    Minimal,
    /// Opt into installed executable directories, never account settings or auth.
    InstalledTools,
}

/// Validation is directory-only. No discovered tool runs during preflight.
#[cfg(windows)]
pub(crate) fn installed_path() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // The installed pnpm shims locate their packages relative to themselves.
    // Prefer them to account wrappers that look under the redirected app data.
    paths.extend(pnpm_installation(
        std::env::var_os("PNPM_HOME").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
    ));
    if let Some(path) = std::env::var_os("PATH") {
        // The active app's account-local CLI bindings target its production host.
        paths.extend(std::env::split_paths(&path).filter(|p| {
            !p.components().any(|part| {
                part.as_os_str()
                    .eq_ignore_ascii_case(OsStr::new("Shellfox"))
            })
        }));
    }
    validated_directories(paths)
}

#[cfg(windows)]
fn pnpm_installation(configured: Option<PathBuf>, local_app_data: Option<PathBuf>) -> Option<PathBuf> {
    configured
        .into_iter()
        .chain(local_app_data.map(|data| data.join("pnpm")))
        .find(|home| {
            home.is_absolute() && (home.join("pi.cmd").is_file() || home.join("pnpm.cmd").is_file())
        })
}

#[cfg(any(windows, target_os = "linux", test))]
pub(crate) fn validated_directories(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !path.is_absolute() || has_nul(path.as_os_str()) || !path.is_dir() {
            continue;
        }
        let Ok(path) = std::fs::canonicalize(path) else {
            continue;
        };
        let path = lookup_path(path);
        if !result.iter().any(|old| same_path(old, &path)) {
            result.push(path);
        }
    }
    result
}

// PowerShell script authorization and Windows PowerShell path providers reject
// verbatim paths. Keep canonical identity, but use ordinary DOS/UNC spelling
// for command lookup, cwd and environment paths.
#[cfg(any(windows, target_os = "linux", test))]
pub(crate) fn lookup_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut parts = path.components();
        let Some(Component::Prefix(prefix)) = parts.next() else {
            return path;
        };
        let mut result = match prefix.kind() {
            Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", char::from(drive))),
            Prefix::VerbatimUNC(server, share) => {
                let mut unc = OsString::from("\\\\");
                unc.push(server);
                unc.push("\\");
                unc.push(share);
                PathBuf::from(unc)
            }
            _ => return path,
        };
        for part in parts {
            if !matches!(part, Component::RootDir) {
                result.push(part.as_os_str());
            }
        }
        result
    }
    #[cfg(not(windows))]
    {
        path
    }
}

#[cfg(any(windows, target_os = "linux", test))]
fn same_path(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

#[cfg(any(windows, target_os = "linux", test))]
pub(crate) fn protected(key: &OsStr) -> bool {
    let name = key.to_string_lossy().to_ascii_uppercase();
    name.starts_with("PI_")
        || name.starts_with("SHELLFOX_")
        || name.starts_with("NPM_CONFIG_")
        || name.starts_with("PNPM_")
        || name.starts_with("XDG_")
        || name.starts_with("DOTNET_")
        || name.starts_with("CORECLR_")
        || name.starts_with("COR_")
        || name.starts_with("COMPLUS_")
        || [
            "PATH",
            "PATHEXT",
            "HOME",
            "USERPROFILE",
            "HOMEDRIVE",
            "HOMEPATH",
            "APPDATA",
            "LOCALAPPDATA",
            "PROGRAMDATA",
            "SYSTEMROOT",
            "WINDIR",
            "COMSPEC",
            "TMP",
            "TEMP",
            "TMPDIR",
            "HISTFILE",
            "PSMODULEPATH",
            "PSREADLINE_HISTORYFILE",
            "ZDOTDIR",
            "BASH_ENV",
            "ENV",
            "SSH_AUTH_SOCK",
            "SSH_AGENT_PID",
            "NODE_OPTIONS",
            "NODE_PATH",
            "PYTHONHOME",
            "PYTHONPATH",
            "RUSTUP_HOME",
            "CARGO_HOME",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG",
            "GIT_CONFIG_PARAMETERS",
            "GIT_SSH",
            "GIT_SSH_COMMAND",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "TERM",
            "COLORTERM",
        ]
        .contains(&name.as_str())
}

#[cfg(any(windows, target_os = "linux", test))]
pub(crate) fn validate_extra(env: &BTreeMap<OsString, OsString>) -> Result<()> {
    for (key, value) in env {
        ensure!(
            !key.is_empty()
                && !key.to_string_lossy().contains('=')
                && !has_nul(key)
                && !has_nul(value),
            "invalid environment entry"
        );
        ensure!(
            !protected(key),
            "environment entry {} is controlled by the isolation policy",
            key.to_string_lossy()
        );
    }
    Ok(())
}

#[cfg(any(windows, target_os = "linux", test))]
pub(crate) fn has_nul(value: &OsStr) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().any(|c| c == 0)
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().contains(&0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn canonical_lookup_paths_keep_ordinary_drive_and_unc_spelling() {
        assert_eq!(
            lookup_path(PathBuf::from(r"\\?\C:\Program Files\Tools")),
            PathBuf::from(r"C:\Program Files\Tools")
        );
        assert_eq!(
            lookup_path(PathBuf::from(r"\\?\UNC\server\share\tools")),
            PathBuf::from(r"\\server\share\tools")
        );
    }

    #[cfg(windows)]
    #[test]
    fn pnpm_installation_falls_back_to_local_app_data_without_a_hint() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("pnpm-discovery-{}", std::process::id()));
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        std::fs::create_dir(&root).unwrap();
        let _scratch = Scratch(root.clone());
        let default = root.join("pnpm");
        let configured = root.join("custom-location");
        for installation in [&default, &configured] {
            std::fs::create_dir(installation).unwrap();
            std::fs::write(installation.join("pi.cmd"), "@exit /b 0\r\n").unwrap();
        }
        assert_eq!(pnpm_installation(None, Some(root.clone())), Some(default.clone()));
        assert_eq!(
            pnpm_installation(Some("relative".into()), Some(root.clone())),
            Some(default)
        );
        assert_eq!(
            pnpm_installation(Some(configured.clone()), Some(root)),
            Some(configured)
        );
    }

    #[test]
    fn reserved_environment_aliases_cannot_restore_account_state_or_hooks() {
        for key in [
            "Path",
            "home",
            "PI_CODING_AGENT_DIR",
            "pi_package_dir",
            "SHELLFOX_USER_DATA",
            "npm_config_userconfig",
            "PNPM_HOME",
            "NODE_OPTIONS",
            "COMPLUS_Version",
            "COMPLUS_ProfAPI_ProfilerCompatibilitySetting",
            "PSModulePath",
            "SSH_AUTH_SOCK",
            "BASH_ENV",
        ] {
            assert!(
                validate_extra(&BTreeMap::from([(key.into(), "bad".into())])).is_err(),
                "{key}"
            );
        }
        assert!(validate_extra(&BTreeMap::from([("CUSTOM_VALUE".into(), "ok".into())])).is_ok());
        assert!(validate_extra(&BTreeMap::from([("BAD".into(), "a\0b".into())])).is_err());
    }
    #[test]
    fn search_paths_drop_empty_relative_missing_and_duplicate_directories() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dirs = validated_directories([
            PathBuf::new(),
            ".".into(),
            "relative".into(),
            root.join("does-not-exist"),
            root.clone(),
            root.clone(),
        ]);
        assert_eq!(
            dirs,
            vec![lookup_path(std::fs::canonicalize(root).unwrap())]
        );
    }
}
