//! A desktop-installed CLI hands GUI work to the desktop binary next to it,
//! as 0.9.x did. Headless packages never start a GUI.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The desktop executable installed with this CLI, if any.
pub fn desktop_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if crate::headless_layout::current_root().is_some() {
        return None;
    }
    sibling_desktop(&exe)
}

/// Windows: `<install>\bin\shellfox-cli.exe` beside `<install>\Shellfox.exe`.
/// Linux: `shellfox` beside `shellfox-cli`. macOS: `Shellfox` in Contents/MacOS.
pub fn sibling_desktop(cli: &Path) -> Option<PathBuf> {
    let dir = cli.parent()?;
    let candidate = if cfg!(windows) {
        let install = if dir
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("bin"))
        {
            dir.parent()?
        } else {
            dir
        };
        install.join("Shellfox.exe")
    } else if cfg!(target_os = "macos") {
        dir.join("Shellfox")
    } else {
        dir.join("shellfox")
    };
    candidate.is_file().then_some(candidate)
}

/// The Rust host publishes this file while it runs.
pub fn endpoint_exists(data: &Path) -> bool {
    data.join("rust-service-v1").join("endpoint.json").exists()
}

/// Cold-start the desktop on a new session in `cwd`, detached from this console.
pub fn open_session(desktop: &Path, cwd: &str) -> Result<()> {
    let mut command = std::process::Command::new(desktop);
    // The desktop outlives this tab; it must not carry the tab's session context.
    command.env_remove("SHELLFOX_TERMINAL_MARKER");
    command
        .arg("start")
        .arg(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .spawn()
        .with_context(|| format!("Could not start {}", desktop.display()))?;
    Ok(())
}

/// Desktop installs update through the desktop updater. Exit codes are its own:
/// 0 done or current, 1 failed or Shellfox running, 2 usage, 3 unsupported install.
/// Explicit inherited handles let the windows-subsystem desktop write to this console.
pub fn update(desktop: &Path, check: bool) -> Result<i32> {
    let mut command = std::process::Command::new(desktop);
    // The desktop outlives this tab; it must not carry the tab's session context.
    command.env_remove("SHELLFOX_TERMINAL_MARKER");
    command.arg("--cli-update");
    if check {
        command.arg("--check");
    }
    let status = command
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Could not start {}", desktop.display()))?;
    Ok(status.code().unwrap_or(1))
}

/// The desktop installed with this CLI installs or repairs one integration.
/// A headless package reports its bundled CLI read-only and has no file-manager
/// provider, as in 0.9.
pub fn ensure_integration(kind: crate::manager::Integration) -> Result<serde_json::Value> {
    use crate::output::CliError;
    if kind == crate::manager::Integration::Cli && crate::headless_layout::current_root().is_some()
    {
        return Ok(headless_cli_state());
    }
    let desktop = desktop_binary().ok_or_else(|| {
        CliError::coded(
            "UNSUPPORTED",
            "This host has no desktop integration provider.",
            false,
        )
    })?;
    let output = std::process::Command::new(&desktop)
        .env_remove("SHELLFOX_TERMINAL_MARKER")
        .args(["--ensure-integration", kind.argument()])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .with_context(|| format!("Could not start {}", desktop.display()))?;
    integration_reply(&output.stdout)
}

/// 0.9's read-only bundled-CLI state, with the headless package's own wording.
fn headless_cli_state() -> serde_json::Value {
    serde_json::json!({
        "supported": false,
        "installed": true,
        "command": "shellfox",
        "reason": "The bundled CLI is in the headless installation folder. Shellfox doesn't change the global PATH.",
    })
}

/// The desktop's 0.9 `Result` JSON: the state, or the error as a CLI error.
fn integration_reply(stdout: &[u8]) -> Result<serde_json::Value> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        ok: bool,
        value: Option<serde_json::Value>,
        error: Option<shellfox_protocol::AppError>,
    }
    let reply: Reply =
        serde_json::from_slice(stdout).context("The desktop returned no integration result")?;
    match reply {
        Reply {
            ok: true,
            value: Some(value),
            error: None,
        } => Ok(value),
        Reply {
            ok: false,
            value: None,
            error: Some(error),
        } => Err(crate::output::CliError::app(error)),
        _ => anyhow::bail!("The desktop returned an invalid integration result"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headless_cli_state_is_installed_and_read_only() {
        let state: shellfox_protocol::CliIntegration =
            serde_json::from_value(headless_cli_state()).unwrap();
        assert!(state.installed && !state.supported);
        assert_eq!(state.command, "shellfox");
    }
    #[test]
    fn integration_replies_carry_the_state_or_the_desktop_error() {
        let state = integration_reply(br#"{"ok":true,"value":{"installed":true}}"#).unwrap();
        assert_eq!(state["installed"], true);
        let error = integration_reply(
            br#"{"ok":false,"error":{"code":"UNSUPPORTED","message":"No file manager.","retryable":false}}"#,
        )
        .unwrap_err();
        let error = error.downcast_ref::<crate::output::CliError>().unwrap();
        assert_eq!(
            (&*error.code, &*error.message),
            ("UNSUPPORTED", "No file manager.")
        );
        assert!(integration_reply(b"").is_err());
        assert!(integration_reply(br#"{"ok":true}"#).is_err());
    }
    #[test]
    fn finds_only_the_installed_sibling_desktop() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let cli = bin.join("shellfox-cli");
        assert_eq!(sibling_desktop(&cli), None);
        let desktop = if cfg!(windows) {
            temp.path().join("Shellfox.exe")
        } else if cfg!(target_os = "macos") {
            bin.join("Shellfox")
        } else {
            bin.join("shellfox")
        };
        std::fs::write(&desktop, b"x").unwrap();
        assert_eq!(sibling_desktop(&cli), Some(desktop));
        let data = temp.path().join("data");
        assert!(!endpoint_exists(&data));
        std::fs::create_dir_all(data.join("rust-service-v1")).unwrap();
        std::fs::write(data.join("rust-service-v1/endpoint.json"), b"{}").unwrap();
        assert!(endpoint_exists(&data));
    }
}
