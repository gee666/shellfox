//! How the desktop host process is spawned on each OS.

pub(super) use os::*;
#[cfg(windows)]
mod os {
    use std::{ffi::OsString, os::windows::process::CommandExt, path::Path, process::Command};
    // A hidden console of its own: its console children never flash a window,
    // and closing the terminal that started the UI does not end the host.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    /// Break away from a job the UI may run in, such as a launcher's kill-on-close job.
    pub fn scope(cli: &Path, arguments: &[OsString]) -> Option<Command> {
        let mut command = direct(cli, arguments);
        command.creation_flags(
            CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB,
        );
        Some(command)
    }
    pub fn direct(cli: &Path, arguments: &[OsString]) -> Command {
        let mut command = Command::new(cli);
        command
            .args(arguments)
            .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
        command
    }
}

#[cfg(unix)]
mod os {
    use std::{ffi::OsString, os::unix::process::CommandExt, path::Path, process::Command};
    unsafe extern "C" {
        fn setsid() -> i32;
    }
    /// Linux: a transient scope of its own with OOMPolicy=continue, so an OOM
    /// kill of the UI (whose app scope systemd may stop) leaves the host alone.
    #[cfg(target_os = "linux")]
    pub fn scope(cli: &Path, arguments: &[OsString]) -> Option<Command> {
        let runner = Path::new("/usr/bin/systemd-run");
        // systemd-run may expand `$` in arguments; keep such paths direct.
        let plain = std::iter::once(cli.as_os_str())
            .chain(arguments.iter().map(OsString::as_os_str))
            .all(|argument| !argument.as_encoded_bytes().contains(&b'$'));
        if !plain || !runner.is_file() {
            return None;
        }
        let mut command = detached(runner);
        command
            .args(scope_arguments(&uuid::Uuid::new_v4()))
            .arg(cli)
            .args(arguments);
        Some(command)
    }
    #[cfg(target_os = "linux")]
    pub fn scope_arguments(unit: &uuid::Uuid) -> Vec<String> {
        vec![
            "--user".into(),
            "--scope".into(),
            "--quiet".into(),
            "--collect".into(),
            format!("--unit=shellfox-host-{unit}"),
            "--description=Shellfox host".into(),
            "--property=OOMPolicy=continue".into(),
            "--".into(),
        ]
    }
    #[cfg(not(target_os = "linux"))]
    pub fn scope(_cli: &Path, _arguments: &[OsString]) -> Option<Command> {
        None
    }
    pub fn direct(cli: &Path, arguments: &[OsString]) -> Command {
        let mut command = detached(cli);
        command.args(arguments);
        command
    }
    /// A session of its own: no SIGHUP when the UI's terminal closes.
    fn detached(program: &Path) -> Command {
        let mut command = Command::new(program);
        // SAFETY: setsid is async-signal-safe and touches no parent state.
        unsafe {
            command.pre_exec(|| {
                if setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command
    }
}
