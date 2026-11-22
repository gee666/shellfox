//! `shellfox update [--check]` on desktop installs. shellfox-cli forwards it as
//! `<desktop> --cli-update [--check]` with its stdio. This runs before any
//! window or host and uses the in-app checks, without the GUI confirmation.
use super::{Feed, Platform};
use semver::Version;
use std::ffi::OsString;

pub(crate) const FLAG: &str = "--cli-update";

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Check,
    Install,
}

fn mode(args: &[OsString]) -> Option<Mode> {
    match args {
        [] => Some(Mode::Install),
        [check] if check == "--check" => Some(Mode::Check),
        _ => None,
    }
}

/// Exit codes: 0 done, 1 failed, 2 usage, 3 unsupported installation.
pub(crate) fn run(args: &[OsString]) -> i32 {
    #[cfg(windows)]
    use_parent_console();
    let Some(mode) = mode(args) else {
        eprintln!("Usage: shellfox update [--check]");
        return 2;
    };
    let current: Version = env!("SHELLFOX_VERSION").parse().expect("product version");
    println!("Installed Shellfox version: {current}");
    let platform = Platform::detect();
    if let Some(reason) = platform.reason() {
        eprintln!("Shellfox: {reason} See {}", super::RELEASES);
        return 3;
    }
    match update(&platform, &current, mode) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Shellfox: {error:#}");
            1
        }
    }
}

fn update(platform: &Platform, current: &Version, mode: Mode) -> anyhow::Result<()> {
    use anyhow::Context;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let feed = Feed::new()?;
    let release = runtime
        .block_on(feed.check(|version| platform.asset_name(version)))
        .with_context(|| {
            format!(
                "could not check the latest release. Check your connection, or see {}",
                super::RELEASES
            )
        })?;
    let latest = &release.version;
    if latest <= current {
        println!("Shellfox is up to date ({current}).");
        return Ok(());
    }
    if mode == Mode::Check {
        println!("Shellfox {latest} is available (installed: {current}). Run: shellfox update");
        return Ok(());
    }
    let asset = release.asset.as_ref().with_context(|| {
        format!(
            "release {latest} has no verified installer for this platform. See {}",
            super::RELEASES
        )
    })?;
    anyhow::ensure!(
        !platform.instance_running()?,
        "Shellfox is running. Use Update in the app, or quit Shellfox and run shellfox update again."
    );
    println!("Updating Shellfox {current} -> {latest}");
    let work = tempfile::Builder::new()
        .prefix("shellfox-update-")
        .tempdir()?;
    let file = work.path().join(&asset.name);
    runtime
        .block_on(feed.download(asset, &file, |_| {}))
        .context("the update could not be downloaded or verified")?;
    let prepared = platform.prepare(file, latest, work, asset)?;
    if prepared.install()? {
        println!("Shellfox {latest} installed.");
    } else {
        println!("Installing Shellfox {latest}. Start Shellfox again in a few seconds.");
    }
    Ok(())
}

/// This GUI-subsystem binary is not attached to the console its console
/// handles come from, so writes through them are lost. Attach to the CLI's
/// console and reopen those handles; pipes and files are kept as they are.
#[cfg(windows)]
fn use_parent_console() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(id: u32) -> *mut c_void;
        fn SetStdHandle(id: u32, handle: *mut c_void) -> i32;
        fn GetFileType(handle: *mut c_void) -> u32;
        fn AttachConsole(process: u32) -> i32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *const c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
    }
    const FILE_TYPE_DISK: u32 = 1;
    const FILE_TYPE_PIPE: u32 = 3;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const GENERIC_READ_WRITE: u32 = 0x8000_0000 | 0x4000_0000;
    const SHARE_READ_WRITE: u32 = 1 | 2;
    const OPEN_EXISTING: u32 = 3;
    let streams = [
        (-10i32 as u32, "CONIN$"),
        (-11i32 as u32, "CONOUT$"),
        (-12i32 as u32, "CONOUT$"),
    ];
    // SAFETY: these calls only inspect or replace this process's std handles.
    unsafe {
        let console: Vec<_> = streams
            .into_iter()
            .filter(|(id, _)| {
                let handle = GetStdHandle(*id);
                handle.is_null() || !matches!(GetFileType(handle), FILE_TYPE_DISK | FILE_TYPE_PIPE)
            })
            .collect();
        if console.is_empty() || AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        for (id, name) in console {
            let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
            let handle = CreateFileW(
                name.as_ptr(),
                GENERIC_READ_WRITE,
                SHARE_READ_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );
            if handle as isize != -1 {
                SetStdHandle(id, handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_update_and_check() {
        assert_eq!(mode(&[]), Some(Mode::Install));
        assert_eq!(mode(&["--check".into()]), Some(Mode::Check));
        assert_eq!(mode(&["--json".into()]), None);
        assert_eq!(mode(&["--check".into(), "--check".into()]), None);
    }
}
