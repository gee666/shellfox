use crate::windows::{open_event, wait, STARTUP_TIMEOUT_MS, SUPERVISOR_FLAG};
use anyhow::{ensure, Context, Result};
use std::ffi::OsString;
use std::os::windows::io::AsRawHandle;
use std::process::Command;
use windows_sys::Win32::System::JobObjects::IsProcessInJob;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, SetEvent, EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE,
};

pub(crate) fn entry() -> Result<Option<u32>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(SUPERVISOR_FLAG)) {
        return Ok(None);
    }
    let release_name = args.next().context("missing release event")?;
    let ready_name = args.next().context("missing ready event")?;
    let release_name = release_name.to_str().context("invalid release event")?;
    let ready_name = ready_name.to_str().context("invalid ready event")?;
    let nonce = release_name
        .strip_prefix("Local\\ShellfoxPty-")
        .and_then(|name| name.strip_suffix("-GO"))
        .context("invalid release event name")?;
    ensure!(
        nonce.len() == 32 && nonce.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid startup nonce"
    );
    ensure!(
        ready_name == format!("Local\\ShellfoxPty-{nonce}-READY"),
        "startup nonce mismatch"
    );
    let proof = crate::inspection_gate::SupervisorProof::parse(&mut args, release_name)?;
    let first = args.next().context("missing supervised program")?;
    let (program_environment, program) =
        crate::program_env_pipe::parse(first, &mut args, release_name)?;
    ensure!(
        std::path::Path::new(&program).is_absolute(),
        "supervised program must be absolute"
    );
    let release = open_event(release_name, SYNCHRONIZATION_SYNCHRONIZE)?;
    let ready = open_event(ready_name, EVENT_MODIFY_STATE)?;
    ensure!(
        wait(release.as_raw_handle().cast(), STARTUP_TIMEOUT_MS)?,
        "supervisor release timed out"
    );
    let mut in_job = 0;
    // The parent separately verifies membership in its exact Job before release.
    let ok = unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) };
    ensure!(
        ok != 0 && in_job != 0,
        "uncontained supervisor refuses to spawn"
    );
    let arguments: Vec<OsString> = args.collect();
    let mut command = Command::new(&program);
    command.args(arguments);
    if let Some(pipe) = program_environment {
        ensure!(
            crate::program_env::wire::KEYS
                .iter()
                .all(|key| std::env::var_os(key).is_none()),
            "Trusted carrier leaked into supervisor environment."
        );
        crate::program_env_pipe::apply(
            &pipe,
            proof.parent_identity(),
            std::path::Path::new(&program),
            &mut command,
        )?;
    }
    let mut child = command.spawn().with_context(|| {
        format!(
            "spawn contained PTY program {}",
            std::path::Path::new(&program).display()
        )
    })?;
    proof.publish(&child)?;
    let ok = unsafe { SetEvent(ready.as_raw_handle().cast()) };
    ensure!(
        ok != 0,
        "signal supervisor ready: {}",
        std::io::Error::last_os_error()
    );
    let status = child.wait().context("wait for contained PTY command")?;
    Ok(Some(status.code().unwrap_or(1) as u32))
}
