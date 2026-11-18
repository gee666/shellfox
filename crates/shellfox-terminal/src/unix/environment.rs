use crate::{validate_isolated_path, EnvironmentPolicy, IsolationPathKind};
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(crate) fn build(
    root: &Path,
    policy: EnvironmentPolicy,
    tools: &[PathBuf],
) -> Result<BTreeMap<OsString, OsString>> {
    let root = root.canonicalize()?;
    validate_isolated_path(&root, &root, IsolationPathKind::Directory)?;
    let mut env = BTreeMap::new();
    let mut paths = vec![
        PathBuf::from("/usr/local/bin"),
        "/usr/bin".into(),
        "/bin".into(),
    ];
    if policy == EnvironmentPolicy::InstalledTools {
        paths.extend_from_slice(tools);
        if let Some(path) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&path));
        }
    }
    env.insert(
        "PATH".into(),
        std::env::join_paths(crate::environment::validated_directories(paths))?,
    );
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());
    env.insert("LANG".into(), "C.UTF-8".into());
    for (directory, keys) in [
        ("home", &["HOME", "ZDOTDIR"][..]),
        ("config", &["XDG_CONFIG_HOME"][..]),
        ("data", &["XDG_DATA_HOME"][..]),
        ("cache", &["XDG_CACHE_HOME", "npm_config_cache"][..]),
        ("state", &["XDG_STATE_HOME"][..]),
        ("temp", &["TMPDIR", "TMP", "TEMP"][..]),
        ("pi-agent", &["PI_CODING_AGENT_DIR"][..]),
        ("pi-sessions", &["PI_CODING_AGENT_SESSION_DIR"][..]),
        ("npm-prefix", &["npm_config_prefix"][..]),
        ("pnpm-home", &["PNPM_HOME"][..]),
    ] {
        let path = root.join(directory);
        validate_isolated_path(&path, &root, IsolationPathKind::Directory)?;
        std::fs::create_dir_all(&path).with_context(|| format!("create isolated {directory}"))?;
        validate_isolated_path(&path, &root, IsolationPathKind::Directory)?;
        for key in keys {
            env.insert((*key).into(), path.as_os_str().to_owned());
        }
    }
    for (key, value) in [
        ("HISTFILE", "state/shell-history"),
        ("GIT_CONFIG_GLOBAL", "config/gitconfig"),
        ("GIT_CONFIG_SYSTEM", "config/gitconfig-system"),
        ("npm_config_userconfig", "config/npmrc"),
        ("npm_config_globalconfig", "config/npm-globalrc"),
    ] {
        let path = root.join(value);
        validate_isolated_path(&path, &root, IsolationPathKind::File)?;
        env.insert(key.into(), path.into_os_string());
    }
    Ok(env)
}
