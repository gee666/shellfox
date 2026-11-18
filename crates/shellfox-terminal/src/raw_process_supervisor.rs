use super::{command::MAX_CONFIG_BYTES, RawCommand};
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::Read,
    os::windows::{
        io::{AsRawHandle, FromRawHandle},
        process::CommandExt,
    },
    process::{Command, Stdio},
};
use windows_sys::Win32::{
    Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS},
    System::{
        JobObjects::IsProcessInJob,
        Threading::{
            GetCurrentProcess, SetEvent, CREATE_NO_WINDOW, EVENT_MODIFY_STATE,
            SYNCHRONIZATION_SYNCHRONIZE,
        },
    },
};

pub(super) const FLAG: &str = "--shellfox-raw-supervisor";
pub(super) fn entry() -> Option<u32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(FLAG)) {
        return None;
    }
    // Never print errors into the target's raw streams.
    Some(run(args).unwrap_or(1))
}
fn run(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<u32> {
    let release = args.next().context("Missing release event.")?;
    let ready = args.next().context("Missing ready event.")?;
    ensure!(
        args.next().is_none(),
        "Unexpected raw supervisor arguments."
    );
    let release = release.to_str().context("Invalid release event.")?;
    let ready = ready.to_str().context("Invalid ready event.")?;
    let nonce = release
        .strip_prefix("Local\\ShellfoxPty-")
        .and_then(|s| s.strip_suffix("-GO"))
        .context("Invalid raw startup event.")?;
    ensure!(
        nonce.len() == 32
            && nonce.bytes().all(|b| b.is_ascii_hexdigit())
            && ready == format!("Local\\ShellfoxPty-{nonce}-READY"),
        "Invalid raw startup nonce."
    );
    let go = crate::windows::open_event(release, SYNCHRONIZATION_SYNCHRONIZE)?;
    let ready = crate::windows::open_event(ready, EVENT_MODIFY_STATE)?;
    ensure!(
        crate::windows::wait(
            go.as_raw_handle().cast(),
            crate::windows::STARTUP_TIMEOUT_MS
        )?,
        "Raw startup timed out."
    );
    let mut contained = 0;
    // Parent checks membership in its exact non-breakaway Job on the original
    // process handle before GO. The supervisor cannot spawn before this check.
    ensure!(
        unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut contained) } != 0
            && contained != 0,
        "Raw supervisor is uncontained."
    );
    let command = read_command()?;
    let mut child = Command::new(&command.executable)
        .args(&command.args)
        .current_dir(&command.cwd)
        .env_clear()
        .envs(&command.env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("Raw target launch failed.")?;
    ensure!(
        unsafe { SetEvent(ready.as_raw_handle().cast()) } != 0,
        "Raw READY failed."
    );
    // The parent still owns the Job and the original supervisor process handle.
    // On this root exit it terminates all remaining descendants before reporting.
    Ok(child.wait()?.code().unwrap_or(1) as u32)
}
fn read_command() -> Result<RawCommand> {
    let process = unsafe { GetCurrentProcess() };
    let source = std::io::stdin().as_raw_handle().cast();
    let mut handle = std::ptr::null_mut();
    ensure!(
        unsafe {
            DuplicateHandle(
                process,
                source,
                process,
                &mut handle,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } != 0,
        "Raw startup stdin unavailable."
    );
    // File performs unbuffered ReadFile. std::io::stdin internally buffers and
    // can steal bytes that belong to the eventual target's stdin.
    let mut input = unsafe { File::from_raw_handle(handle.cast()) };
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure!(
        (1..=MAX_CONFIG_BYTES).contains(&length),
        "Raw startup frame exceeds its bound."
    );
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    // serde errors may contain input fragments, so never propagate the parser's
    // text to any logger or host response.
    let command: RawCommand = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid raw startup frame."))?;
    command.validate()?;
    ensure!(
        std::path::Path::new(&command.executable).is_absolute(),
        "Raw startup executable must be resolved."
    );
    Ok(command)
}
