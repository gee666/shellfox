use anyhow::{Context, Result};
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::{ffi::OsString, process::Command};
#[cfg(any(windows, target_os = "macos"))]
use {anyhow::ensure, std::path::Path};

/// Portable installations have no PATH shim. The desktop binary still supports
/// the same route before it initializes GPUI, a host, or any native services.
pub(crate) fn forward_ssh(args: &[OsString]) -> Result<Option<i32>> {
    if !args.first().is_some_and(|arg| arg == "ssh") {
        return Ok(None);
    }
    let executable = std::env::current_exe()?;
    let bin = executable
        .parent()
        .context("Missing Shellfox executable directory")?;
    let cli = bin.join(if cfg!(windows) {
        "bin/shellfox-cli.exe"
    } else {
        "shellfox-cli"
    });
    let cli = if cli.is_file() {
        cli
    } else {
        bin.join(if cfg!(windows) {
            "shellfox-cli.exe"
        } else {
            "shellfox-cli"
        })
    };
    let mut command = Command::new(cli);
    command.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec()).context("Unable to launch shellfox-cli")
    }
    #[cfg(not(unix))]
    {
        let status = command.status().context("Unable to launch shellfox-cli")?;
        Ok(Some(status.code().unwrap_or(1)))
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn install_file(
    path: &Path,
    contents: Option<&str>,
    owner: &str,
    legacy: impl FnOnce(&str) -> bool,
) -> Result<()> {
    use std::{
        fs,
        io::{ErrorKind, Write},
    };
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                meta.is_file() && !meta.is_symlink() && meta.len() <= 128 * 1024,
                "Foreign Shellfox launcher: {}",
                path.display()
            );
            let existing = fs::read_to_string(path)?;
            ensure!(
                existing.lines().any(|line| line == owner)
                    || (contents.is_some() && legacy(&existing)),
                "Foreign Shellfox launcher: {}",
                path.display()
            );
            if contents == Some(existing.as_str()) {
                return Ok(());
            }
            if contents.is_none() {
                return Ok(fs::remove_file(path)?);
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if contents.is_none() {
                return Ok(());
            }
        }
        Err(error) => return Err(error.into()),
    }
    let parent = path
        .parent()
        .context("Missing Shellfox launcher directory")?;
    fs::create_dir_all(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    if let Some(contents) = contents {
        pending.write_all(contents.as_bytes())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        pending
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    pending.persist(path)?;
    Ok(())
}

/// Writes the `shellfox` launchers that Squirrel installs put on the user PATH.
/// Electron 0.9.15 owned the same directory; its Node shims are replaced and
/// its leftover helper files removed once its ownership marker is verified.
#[cfg(windows)]
pub(crate) fn install_windows_shims(
    bin: &Path,
    install: &Path,
    version_dir: &Path,
    exe: &Path,
    remove: bool,
) -> Result<()> {
    let legacy = electron_shim_owner(bin, install);
    let exe = exe
        .to_str()
        .context("Invalid Shellfox executable encoding")?;
    let cli = version_dir.join("bin/shellfox-cli.exe");
    let cli = cli.to_str().context("Invalid Shellfox CLI encoding")?;
    let shims = windows_shims(exe, cli);
    let allow_legacy = !remove && legacy;
    for (name, contents, owner) in [
        ("shellfox.cmd", shims.cmd, "rem Shellfox/rust-cli-v1"),
        ("shellfox.ps1", shims.powershell, "# Shellfox/rust-cli-v1"),
        ("shellfox", shims.posix, "# Shellfox/rust-cli-v1"),
    ] {
        install_file(
            &bin.join(name),
            (!remove).then_some(contents.as_str()),
            owner,
            |_| allow_legacy,
        )?;
    }
    if legacy {
        // The marker goes last, so a failed cleanup is retried on the next hook.
        for name in [
            "shellfox-dispatch.ps1",
            "shellfox-update.ps1",
            "shellfox-owner.json",
        ] {
            match std::fs::remove_file(bin.join(name)) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn electron_shim_owner(bin: &Path, install: &Path) -> bool {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Owner {
        owner: String,
        executable: std::path::PathBuf,
        app_path: Option<std::path::PathBuf>,
    }
    let marker = bin.join("shellfox-owner.json");
    if !std::fs::symlink_metadata(&marker)
        .is_ok_and(|info| info.is_file() && !info.is_symlink() && info.len() <= 4096)
    {
        return false;
    }
    let Ok(bytes) = std::fs::read(marker) else {
        return false;
    };
    let Ok(owner) = serde_json::from_slice::<Owner>(&bytes) else {
        return false;
    };
    owner.owner == "Shellfox/cli-v1"
        && owner.app_path.is_none()
        && owner.executable.ancestors().nth(2).is_some_and(|root| {
            root.to_string_lossy()
                .eq_ignore_ascii_case(&install.to_string_lossy())
        })
}

#[cfg(any(windows, test))]
struct WindowsShims {
    cmd: String,
    powershell: String,
    posix: String,
}

/// First words that open the desktop instead of running the CLI, as in 0.9.
#[cfg(any(windows, target_os = "macos", test))]
const DESKTOP_VERBS: [&str; 6] = [
    "start",
    "add",
    "new",
    "create",
    "--new-session",
    "--new-window",
];

/// POSIX tail shared by the Git Bash/WSL and macOS shims; `$exe` and `$cli` are set.
#[cfg(any(windows, target_os = "macos", test))]
fn posix_router() -> String {
    format!(
        "case \"${{1:-}}\" in\n  {})\n    \"$exe\" \"$@\" </dev/null >/dev/null 2>&1 &\n    exit 0\n    ;;\nesac\nexec \"$cli\" \"$@\"\n",
        DESKTOP_VERBS.join("|")
    )
}

/// From WSL, the folder of `start|add|new|create [PATH]` and `session create
/// [PATH]` (default: the current one) goes to Windows as wslpath -w spells it,
/// `\\wsl.localhost\<distro>\...`, as 0.9 did. Windows paths pass unchanged.
#[cfg(any(windows, test))]
const WSL_FOLDER: &str = r#"  windows_path() {
    case "$1" in
      [A-Za-z]:[\\/]*|\\\\*) printf '%s\n' "$1" ;;
      *)
        case "$1" in /*) path=$1 ;; *) path=$PWD/$1 ;; esac
        wslpath -w "$(realpath -ms -- "$path" 2>/dev/null || printf '%s' "$path")"
        ;;
    esac
  }
  verbs=0
  case "${1:-}" in
    start|add|new|create) verbs=1 ;;
    session|sessions|ss) [ "${2:-}" != create ] || verbs=2 ;;
  esac
  if [ "$verbs" -gt 0 ]; then
    count=$# index=0 folder= value=
    for arg; do
      index=$((index + 1))
      if [ "$index" -le "$verbs" ] || [ -n "$value" ]; then
        value=
      elif [ "$verbs" = 2 ] && [ "${arg#--}" != "$arg" ] && [ "${arg#*=}" = "$arg" ]; then
        value=1
      elif [ "${arg#-}" = "$arg" ]; then
        folder=$index
        break
      fi
    done
    index=0
    while [ "$index" -lt "$count" ]; do
      arg=$1
      shift
      index=$((index + 1))
      if [ "$index" = "$folder" ]; then arg=$(windows_path "$arg") || exit 1; fi
      set -- "$@" "$arg"
      if [ -z "$folder" ] && [ "$index" = "$verbs" ]; then
        here=$(windows_path .) || exit 1
        set -- "$@" "$here"
      fi
    done
  fi
"#;

#[cfg(any(windows, test))]
fn windows_shims(exe: &str, cli: &str) -> WindowsShims {
    let cmd_quote = |s: &str| s.replace('%', "%%");
    let ps_quote = |s: &str| s.replace('\'', "''");
    let sh_quote = |s: &str| format!("'{}'", s.replace('\\', "/").replace('\'', "'\\''"));
    let (exe_cmd, cli_cmd) = (cmd_quote(exe), cmd_quote(cli));
    let (exe_ps, cli_ps) = (ps_quote(exe), ps_quote(cli));
    let (exe_sh, cli_sh) = (sh_quote(exe), sh_quote(cli));
    let verbs_ps = DESKTOP_VERBS.map(|verb| format!("'{verb}'")).join(",");
    WindowsShims {
        // `start` keeps the console free while the GUI runs.
        cmd: format!(
            "@echo off\r\nrem Shellfox/rust-cli-v1\r\nsetlocal DisableDelayedExpansion\r\nfor %%A in ({}) do if /i \"%~1\"==\"%%A\" (\r\n  start \"\" \"{exe_cmd}\" %*\r\n  exit /b 0\r\n)\r\n\"{cli_cmd}\" %*\r\nexit /b %errorlevel%\r\n",
            DESKTOP_VERBS.join(" ")
        ),
        // PowerShell does not wait for GUI programs.
        powershell: format!(
            "# Shellfox/rust-cli-v1\nif ($args.Count -gt 0 -and @({verbs_ps}) -contains $args[0]) {{ & '{exe_ps}' @args; exit 0 }}\n& '{cli_ps}' @args\nexit $LASTEXITCODE\n"
        ),
        // Git Bash and MSYS run C:/ paths directly. WSL reaches this file
        // through the PATH block Electron added to its startup files.
        posix: format!(
            "#!/bin/sh\n# Shellfox/rust-cli-v1\nexe={exe_sh}\ncli={cli_sh}\nif [ -n \"${{WSL_DISTRO_NAME:-}}\" ]; then\n  exe=$(wslpath -u \"$exe\") && cli=$(wslpath -u \"$cli\") || exit 1\n{WSL_FOLDER}fi\n{}",
            posix_router()
        ),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn install_mac() -> Result<()> {
    if !shellfox_service::app_paths::is_packaged() {
        return Ok(());
    }
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    let executable = std::env::current_exe()?;
    let bundle = executable
        .ancestors()
        .nth(3)
        .context("Invalid Shellfox app bundle path")?;
    // Don't leave a broken launcher when macOS runs the app from a mounted DMG
    // or from its temporary App Translocation directory.
    if !bundle
        .parent()
        .is_some_and(|dir| dir == Path::new("/Applications") || dir == home.join("Applications"))
    {
        return Ok(());
    }
    let quote = |p: PathBuf| -> Result<String> {
        let name = p.to_str().context("Invalid Shellfox bundle encoding")?;
        ensure!(!name.contains(['\n', '\r']), "Invalid Shellfox bundle path");
        Ok(format!("'{}'", name.replace('\'', "'\\''")))
    };
    let desktop = quote(executable.clone())?;
    let cli = quote(executable.with_file_name("shellfox-cli"))?;
    let owner = "# Shellfox/rust-cli-v1";
    let script = format!(
        "#!/bin/sh\n{owner}\nexe={desktop}\ncli={cli}\n{}",
        posix_router()
    );
    install_file(
        &home.join(".local/bin/shellfox"),
        Some(&script),
        owner,
        |existing| existing.lines().any(|line| line == "# Shellfox/linux-v1"),
    )
}

#[cfg(test)]
#[path = "cli_launcher_tests.rs"]
mod tests;
