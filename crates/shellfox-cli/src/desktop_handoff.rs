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

#[cfg(test)]
mod tests {
    use super::*;
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
