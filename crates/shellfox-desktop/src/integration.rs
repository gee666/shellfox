//! Always-on desktop integration, as in 0.9: the `shellfox` command and the
//! "Open in Shellfox" file-manager entries are installed and repaired on every
//! launch. There is no toggle; uninstalling the app removes them.
//! Windows: Squirrel shims, user PATH and Explorer verbs. Linux: per-user
//! Files, Nemo, Dolphin, Thunar and Caja menus that run the .deb's
//! /usr/bin/shellfox, or the ~/.local/bin/shellfox launcher the portable
//! .tar.gz writes. macOS: ~/.local/bin/shellfox; Finder has no menu, as in 0.9.
use serde_json::{Value, json};
use shellfox_protocol::{AppError, CliIntegration, ErrorCode, ExplorerIntegration};

/// `shellfox api setCliIntegration|setExplorerIntegration` runs the desktop
/// with this flag; it prints the 0.9 `Result` JSON.
pub(crate) const FLAG: &str = "--ensure-integration";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Cli,
    Explorer,
}

/// Repairs both integrations off the UI thread; failures are only logged.
pub(crate) fn repair_in_background() {
    let spawned = std::thread::Builder::new()
        .name("desktop-integration".into())
        .spawn(|| {
            for kind in [Kind::Cli, Kind::Explorer] {
                match ensure(kind) {
                    Err(error) if error.code != ErrorCode::Unsupported => {
                        eprintln!("Desktop integration ({kind:?}): {}", error.message);
                    }
                    _ => {}
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("Desktop integration repair could not start: {error}");
    }
}

/// Runs [`FLAG`] and returns the exit code.
pub(crate) fn run_flag(args: &[std::ffi::OsString]) -> i32 {
    let kind = match args.first().and_then(|arg| arg.to_str()) {
        Some("cli") if args.len() == 1 => Kind::Cli,
        Some("explorer") if args.len() == 1 => Kind::Explorer,
        _ => {
            eprintln!("Usage: {FLAG} cli|explorer");
            return 2;
        }
    };
    let reply = match ensure(kind) {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    println!("{reply}");
    0
}

fn ensure(kind: Kind) -> Result<Value, AppError> {
    let value = match kind {
        Kind::Cli => serde_json::to_value(ensure_cli()?),
        Kind::Explorer => serde_json::to_value(ensure_explorer()?),
    };
    value.map_err(|_| failed("Integration state could not be encoded."))
}

fn unsupported(message: &str) -> AppError {
    AppError {
        code: ErrorCode::Unsupported,
        message: message.into(),
        retryable: false,
    }
}
fn failed(message: impl Into<String>) -> AppError {
    AppError {
        code: ErrorCode::NativeUnavailable,
        message: message.into(),
        retryable: true,
    }
}
#[cfg(any(windows, target_os = "macos"))]
fn cli_state(installed: bool) -> CliIntegration {
    CliIntegration {
        supported: true,
        installed,
        command: "shellfox".into(),
        reason: None,
    }
}

#[cfg(windows)]
fn ensure_cli() -> Result<CliIntegration, AppError> {
    if !shellfox_service::app_paths::is_packaged() {
        return Err(unsupported(
            "Shellfox CLI integration requires the installed app.",
        ));
    }
    let shared =
        crate::squirrel::repair(Kind::Cli).map_err(|error| failed(format!("{error:#}")))?;
    Ok(cli_state(shared))
}
#[cfg(windows)]
fn ensure_explorer() -> Result<ExplorerIntegration, AppError> {
    if !shellfox_service::app_paths::is_packaged() {
        return Err(unsupported(
            "Explorer integration requires the installed app.",
        ));
    }
    let shared =
        crate::squirrel::repair(Kind::Explorer).map_err(|error| failed(format!("{error:#}")))?;
    if !shared {
        return Err(unsupported(
            "Test installations never register Explorer verbs.",
        ));
    }
    Ok(ExplorerIntegration {
        supported: true,
        installed: true,
        folder_item_installed: true,
        background_installed: true,
        reason: Some("Windows 11: use Show more options (Shift+F10) in Explorer.".into()),
    })
}

#[cfg(target_os = "linux")]
fn linux() -> Result<shellfox_service::system::HostSystemServices, AppError> {
    use shellfox_service::system::{
        DebPackage, HostSystemServices, NativeSystemEnvironment, PortableLinux,
    };
    use std::sync::Arc;
    let environment = Arc::new(NativeSystemEnvironment);
    let backend = if let Some(package) = DebPackage::current() {
        package.backend(environment)
    } else {
        let exe = std::env::current_exe().map_err(|error| failed(error.to_string()))?;
        shellfox_service::app_paths::is_packaged()
            .then(|| PortableLinux::current(&exe, &*environment))
            .flatten()
            .ok_or_else(|| unsupported("Desktop integration requires the installed app."))?
            .backend(environment)
    };
    Ok(HostSystemServices::new(Arc::new(backend)))
}
#[cfg(target_os = "linux")]
fn ensure_cli() -> Result<CliIntegration, AppError> {
    linux()?.ensure_cli()
}
#[cfg(target_os = "linux")]
fn ensure_explorer() -> Result<ExplorerIntegration, AppError> {
    linux()?.ensure_explorer()
}

#[cfg(target_os = "macos")]
fn ensure_cli() -> Result<CliIntegration, AppError> {
    match crate::cli_launcher::install_mac() {
        Ok(true) => Ok(cli_state(true)),
        Ok(false) => Err(unsupported(
            "Move Shellfox to Applications to install the shellfox command.",
        )),
        Err(error) => Err(failed(format!("{error:#}"))),
    }
}
#[cfg(target_os = "macos")]
fn ensure_explorer() -> Result<ExplorerIntegration, AppError> {
    Err(unsupported("Finder integration is unsupported."))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn ensure_cli() -> Result<CliIntegration, AppError> {
    Err(unsupported("Shellfox CLI integration is unavailable."))
}
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn ensure_explorer() -> Result<ExplorerIntegration, AppError> {
    Err(unsupported(
        "File-manager integration is unavailable on this platform.",
    ))
}
