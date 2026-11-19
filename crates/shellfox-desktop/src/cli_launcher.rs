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

#[cfg(windows)]
pub(crate) fn squirrel(event: &OsString) -> Result<bool> {
    use winreg::{
        RegKey, RegValue,
        enums::{HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ},
    };
    let Some(event) = event.to_str().filter(|arg| {
        matches!(
            *arg,
            "--squirrel-install"
                | "--squirrel-updated"
                | "--squirrel-uninstall"
                | "--squirrel-obsolete"
        )
    }) else {
        return Ok(false);
    };
    if event == "--squirrel-obsolete" || !shellfox_service::app_paths::is_packaged() {
        return Ok(true);
    }
    let exe = std::env::current_exe()?;
    let version_dir = exe.parent().context("Missing Squirrel version directory")?;
    let install = version_dir
        .parent()
        .context("Missing Squirrel install directory")?;
    // Do not change the user's integration for portable or isolated test builds.
    if !install
        .file_name()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("Shellfox"))
        || !install.join("Update.exe").is_file()
    {
        return Ok(true);
    }
    let bin = install.join("bin");
    let remove = event == "--squirrel-uninstall";
    let legacy = !remove && electron_shim_owner(&bin, install);
    let exe = exe
        .to_str()
        .context("Invalid Shellfox executable encoding")?;
    let cli = version_dir.join("bin/shellfox-cli.exe");
    let cli = cli.to_str().context("Invalid Shellfox CLI encoding")?;
    let (cmd, ps) = windows_shims(exe, cli);
    let posix_quote =
        |value: &str| format!("'{}'", value.replace('\\', "/").replace('\'', "'\\''"));
    let posix = format!(
        "#!/bin/sh\n# Shellfox/rust-cli-v1\nif [ \"${{1:-}}\" = ssh ]; then\n  exec {} \"$@\"\nfi\nexec {} \"$@\"\n",
        posix_quote(cli),
        posix_quote(exe),
    );
    install_file(
        &bin.join("shellfox.cmd"),
        (!remove).then_some(cmd.as_str()),
        "rem Shellfox/rust-cli-v1",
        |_| legacy,
    )?;
    install_file(
        &bin.join("shellfox.ps1"),
        (!remove).then_some(ps.as_str()),
        "# Shellfox/rust-cli-v1",
        |_| legacy,
    )?;
    install_file(
        &bin.join("shellfox"),
        (!remove).then_some(posix.as_str()),
        "# Shellfox/rust-cli-v1",
        |_| legacy,
    )?;
    let (env, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Environment")?;
    let (value, kind) = match env.get_raw_value("Path") {
        Ok(raw) => {
            ensure!(
                matches!(raw.vtype, REG_EXPAND_SZ | REG_SZ),
                "Unsupported user PATH registry type"
            );
            (String::from_reg_value(&raw)?, raw.vtype)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (String::new(), REG_EXPAND_SZ)
        }
        Err(error) => return Err(error.into()),
    };
    use winreg::types::{FromRegValue, ToRegValue};
    let path = bin.to_str().context("Invalid Shellfox PATH encoding")?;
    let normalized = |p: &str| {
        p.trim()
            .trim_matches('"')
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    let path_key = normalized(path);
    let mut parts: Vec<_> = value
        .split(';')
        .filter(|p| normalized(p) != path_key)
        .collect();
    if !remove {
        if value.is_empty() {
            parts.clear();
        }
        parts.push(path);
    }
    let joined = parts.join(";");
    let mut new: RegValue = joined.to_reg_value();
    new.vtype = kind;
    env.set_raw_value("Path", &new)?;
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, WPARAM},
            UI::WindowsAndMessaging::{SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE},
        },
        core::w,
    };
    // SAFETY: the literal is valid and remains alive for the synchronous broadcast.
    unsafe {
        SendMessageTimeoutW(
            HWND(0xffff as _),
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("Environment").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        )
    };
    Ok(true)
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
fn windows_shims(exe: &str, cli: &str) -> (String, String) {
    let cmd_quote = |s: &str| s.replace('%', "%%");
    let ps_quote = |s: &str| s.replace('\'', "''");
    let exe_cmd = cmd_quote(exe);
    let (exe_ps, cli_ps) = (ps_quote(exe), ps_quote(cli));
    (
        format!(
            "@echo off\r\nrem Shellfox/rust-cli-v1\r\nsetlocal DisableDelayedExpansion\r\n\"{exe_cmd}\" %*\r\nexit /b %errorlevel%\r\n"
        ),
        format!(
            "# Shellfox/rust-cli-v1\nif ($args.Count -gt 0 -and $args[0] -ceq 'ssh') {{ & '{cli_ps}' @args }} else {{ & '{exe_ps}' @args }}\nexit $LASTEXITCODE\n"
        ),
    )
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
        "#!/bin/sh\n{owner}\nif [ \"${{1:-}}\" = ssh ]; then\n  exec {cli} \"$@\"\nfi\nexec {desktop} \"$@\"\n"
    );
    install_file(
        &home.join(".local/bin/shellfox"),
        Some(&script),
        owner,
        |existing| existing.lines().any(|line| line == "# Shellfox/linux-v1"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shims_forward_ssh_and_preserve_exit_status() {
        let (cmd, ps) = windows_shims(
            r"C:\Program Files\Shellfox.exe",
            r"C:\O'Brien%\bin\shellfox-cli.exe",
        );
        assert!(cmd.contains(r#""C:\Program Files\Shellfox.exe" %*"#));
        assert!(cmd.ends_with("exit /b %errorlevel%\r\n"));
        assert!(ps.contains(r"'C:\O''Brien%\bin\shellfox-cli.exe' @args"));
        assert!(ps.ends_with("exit $LASTEXITCODE\n"));
    }
}
