use crate::{
    environment::{installed_path, lookup_path, validated_directories},
    validate_isolated_path, EnvironmentPolicy, IsolationPathKind,
};
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

pub(crate) fn build(
    root: &Path,
    policy: EnvironmentPolicy,
    tool_directories: &[PathBuf],
) -> Result<BTreeMap<OsString, OsString>> {
    let root = lookup_path(root.to_path_buf());
    let root = root.as_path();
    let canonical_root = std::fs::canonicalize(root).context("resolve isolation root")?;
    validate_isolated_path(root, &canonical_root, IsolationPathKind::Directory)?;
    let mut env = BTreeMap::new();
    let mut set = |key: &str, value: &OsStr| {
        env.insert(key.into(), value.to_owned());
    };
    let system = crate::windows::system_directory()?;
    let windows = system.parent().context("system directory has no parent")?;
    set("SystemRoot", windows.as_os_str());
    set("WINDIR", windows.as_os_str());
    set("COMSPEC", system.join("cmd.exe").as_os_str());
    set("PATHEXT", OsStr::new(".COM;.EXE;.BAT;.CMD"));
    let mut paths = vec![
        system.clone(),
        windows.to_path_buf(),
        system.join("WindowsPowerShell/v1.0"),
        system.join("Wbem"),
    ];
    if policy == EnvironmentPolicy::InstalledTools {
        paths.extend_from_slice(tool_directories);
        paths.extend(installed_path());
    }
    let path =
        std::env::join_paths(validated_directories(paths)).context("assemble executable PATH")?;
    set("PATH", &path);
    set("TERM", OsStr::new("xterm-256color"));
    set("COLORTERM", OsStr::new("truecolor"));
    // PowerShell KnownFolder lookup is not redirected by HOME. Explicitly limit
    // module lookup to the selected installations, never account modules.
    let mut modules = vec![system.join("WindowsPowerShell/v1.0/Modules")];
    if let Some(pwsh) = crate::discover_shell_profiles()?
        .into_iter()
        .find(|p| p.id() == crate::ShellProfileId::Pwsh)
        .and_then(|p| p.executable().map(Path::to_path_buf))
    {
        if let Some(parent) = pwsh.parent() {
            modules.push(parent.join("Modules"));
        }
    }
    let modules = std::env::join_paths(validated_directories(modules))
        .context("assemble isolated module paths")?;
    set("PSModulePath", &modules);
    for (directory, keys) in [
        ("home", &["HOME", "USERPROFILE", "ZDOTDIR"][..]),
        ("config", &["APPDATA", "XDG_CONFIG_HOME"][..]),
        ("data", &["LOCALAPPDATA", "XDG_DATA_HOME"][..]),
        ("cache", &["XDG_CACHE_HOME", "npm_config_cache"][..]),
        ("state", &["XDG_STATE_HOME"][..]),
        ("temp", &["TMP", "TEMP", "TMPDIR"][..]),
        ("pi-agent", &["PI_CODING_AGENT_DIR"][..]),
        ("pi-sessions", &["PI_CODING_AGENT_SESSION_DIR"][..]),
        ("npm-prefix", &["npm_config_prefix"][..]),
        ("pnpm-home", &["PNPM_HOME"][..]),
    ] {
        let path = root.join(directory);
        validate_isolated_path(&path, &canonical_root, IsolationPathKind::Directory)?;
        std::fs::create_dir_all(&path).with_context(|| format!("create isolated {directory}"))?;
        validate_isolated_path(&path, &canonical_root, IsolationPathKind::Directory)?;
        for key in keys {
            set(key, path.as_os_str());
        }
    }
    for (key, path) in [
        ("HISTFILE", "state/shell-history"),
        ("PSREADLINE_HISTORYFILE", "state/powershell-history"),
        ("npm_config_userconfig", "config/npmrc"),
        ("npm_config_globalconfig", "config/npm-globalrc"),
        ("GIT_CONFIG_GLOBAL", "config/gitconfig"),
        ("GIT_CONFIG_SYSTEM", "config/gitconfig-system"),
    ] {
        let path = root.join(path);
        validate_isolated_path(&path, &canonical_root, IsolationPathKind::File)?;
        set(key, path.as_os_str());
    }
    // No inherited hooks, agent sockets, provider credentials, package overrides,
    // PSModulePath, host bindings or WSLENV enter this fresh environment.
    Ok(env)
}
