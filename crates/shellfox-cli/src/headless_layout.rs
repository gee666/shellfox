//! Headless launch forwarding. The package layout itself lives in
//! `shellfox_service::headless_layout`, which the host also reads.
use anyhow::{bail, Context, Result};
pub use shellfox_service::headless_layout::*;
use std::{ffi::OsString, path::Path};

/// The 0.9.x launcher runs `bin/node app/main.cjs ARGS`; `bin/node` is this
/// CLI in Rust packages. Drop the entry argument it adds. Transition-only.
pub fn strip_node_entry(args: &mut Vec<OsString>) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if exe.file_stem().is_none_or(|stem| stem != "node") {
        return;
    }
    let Some(root) = exe.parent().and_then(Path::parent) else {
        return;
    };
    let entry = normalize(&root.join("app").join("main.cjs"));
    if args
        .first()
        .is_some_and(|first| same(&normalize(Path::new(first)), &entry))
    {
        args.remove(0);
    }
}

/// Forward to the selected version, like the 0.9.x launcher did. Unix replaces
/// this process; Windows waits and the child watches this process.
pub fn forward(args: &[OsString]) -> Result<Option<i32>> {
    let Some(root) = current_root() else {
        return Ok(None);
    };
    let Some(target) = active(&root)? else {
        return Ok(None);
    };
    let build = read_build(&target)?;
    if build.platform != platform() || build.arch != arch() {
        bail!("Activated headless runtime does not match this machine.");
    }
    let cli = target.join("bin").join(exe("shellfox-cli"));
    let mut command = std::process::Command::new(&cli);
    command.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec()).with_context(|| format!("Unable to launch {}", cli.display()))
    }
    #[cfg(not(unix))]
    {
        command.env(
            crate::parent_watch::FORWARDER,
            std::process::id().to_string(),
        );
        // Console Ctrl+C reaches both processes; the child decides how to stop.
        crate::parent_watch::ignore_console_interrupts();
        let status = command
            .status()
            .with_context(|| format!("Unable to launch {}", cli.display()))?;
        Ok(Some(status.code().unwrap_or(1)))
    }
}
