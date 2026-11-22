//! The UI is a client of a separate host process, `shellfox-cli --desktop-host`
//! (see shellfox-cli/src/desktop_host.rs), so a UI crash leaves every shell
//! running. The host relaunches a crashed UI once; later launches reattach.
use crate::host_client::HostEndpointConfig;
use anyhow::{Context, Result, bail};
use shellfox_service::{
    desktop_launch::{DesktopLaunchReply, DesktopLaunchRequest, LaunchOwnerFence, LaunchRouteInfo},
    local::{LocalBootstrap, LocalClient},
};
use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Write end of the started host's stdin, kept open for this process's life.
static STARTED: Mutex<Option<ChildStdin>> = Mutex::new(None);
static QUITTING: AtomicBool = AtomicBool::new(false);
const OWNER_MARKER: &str = "desktop-host-owner";

pub(crate) struct Connected {
    pub client: LocalClient,
    pub route: LaunchRouteInfo,
    /// This UI started the host, so its Quit stops it.
    pub started: bool,
}

/// Attach to the data directory's host, starting one if none answers. A host
/// started by a concurrent launch wins the directory lock; ours then exits.
pub(crate) fn connect_or_start(
    runtime: &tokio::runtime::Runtime,
    relaunch_args: &[OsString],
    application_args: &[OsString],
    config: &HostEndpointConfig,
) -> Result<Connected> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut started = false;
    loop {
        let attempt = runtime.block_on(discover(config));
        let error = match attempt {
            Ok((client, route)) => return Ok(Connected { client, route, started }),
            Err(error) => error,
        };
        if started || Instant::now() >= deadline {
            return Err(error.context("The Shellfox host did not answer"));
        }
        match start(relaunch_args, application_args, config) {
            Ok(()) => started = true,
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

async fn discover(config: &HostEndpointConfig) -> Result<(LocalClient, LaunchRouteInfo)> {
    let client = LocalClient::connect(&config.data_dir, &config.helper, config.helper_sha256).await?;
    let DesktopLaunchReply::Route(route) = client.request(&DesktopLaunchRequest::Discover {}).await? else {
        bail!("The host returned an invalid desktop discovery response.");
    };
    Ok((client, route))
}

/// Starts the host and waits until its local API admits clients.
fn start(relaunch_args: &[OsString], application_args: &[OsString], config: &HostEndpointConfig) -> Result<()> {
    let cli = cli_path()?;
    let mut arguments = vec![LocalBootstrap::DESKTOP_HOST_FLAG.into(), std::env::current_exe()?.into_os_string()];
    arguments.push(relaunch_args.len().to_string().into());
    arguments.extend(relaunch_args.iter().cloned());
    arguments.extend(["--data-dir".into(), config.data_dir.clone().into_os_string()]);
    arguments.extend(["--helper".into(), config.helper.clone().into_os_string()]);
    arguments.extend(["--helper-sha256".into(), hex(&config.helper_sha256).into()]);
    arguments.extend(application_args.iter().cloned());
    let output = match platform::scope(&cli, &arguments) {
        Some(mut command) => spawn(&mut command).ok().flatten(),
        None => None,
    };
    let output = match output {
        Some(line) => line,
        // No systemd user manager or no job breakaway: the wrapper failed before the host ran.
        None => spawn(&mut platform::direct(&cli, &arguments))?
            .context("The Shellfox host exited during startup")?,
    };
    if output == LocalBootstrap::DESKTOP_HOST_READY {
        return Ok(());
    }
    bail!("The Shellfox host did not start: {output}")
}

/// Returns the host's first stdout line, or None if it printed nothing.
fn spawn(command: &mut Command) -> Result<Option<String>> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Released hosts outlive this UI and any terminal it was started from.
        .stderr(if cfg!(debug_assertions) { Stdio::inherit() } else { Stdio::null() });
    let mut child = command.spawn().context("Start the Shellfox host")?;
    let stdout = child.stdout.take().context("Host output missing")?;
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line)?;
    let line = line.trim_end().to_owned();
    if line == LocalBootstrap::DESKTOP_HOST_READY {
        *STARTED.lock().unwrap_or_else(|error| error.into_inner()) = child.stdin.take();
        // Not waited: the host outlives this UI by design.
        return Ok(Some(line));
    }
    let _ = child.wait();
    Ok((!line.is_empty()).then_some(line))
}

/// Call before this UI stops the host it started, so the host does not take
/// the UI's exit for a crash and relaunch it.
pub(crate) fn announce_quit() {
    QUITTING.store(true, Ordering::SeqCst);
    if let Some(stdin) = STARTED.lock().unwrap_or_else(|error| error.into_inner()).as_mut() {
        let _ = stdin.write_all(&[LocalBootstrap::DESKTOP_HOST_QUIT]);
        let _ = stdin.flush();
    }
}

/// This UI is stopping its host, so the host's end is expected.
pub(crate) fn quit_announced() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Remembers which host a desktop UI started, so a UI that reattaches after a
/// crash also stops it on Quit. A headless `host run` host never matches.
pub(crate) fn record_owner(desktop_dir: &Path, owner: LaunchOwnerFence) -> Result<()> {
    let mut pending = tempfile::NamedTempFile::new_in(desktop_dir)?;
    pending.write_all(owner.to_string().as_bytes())?;
    pending.persist(desktop_dir.join(OWNER_MARKER))?;
    Ok(())
}
pub(crate) fn recorded_owner(desktop_dir: &Path) -> Option<LaunchOwnerFence> {
    std::fs::read_to_string(desktop_dir.join(OWNER_MARKER)).ok()?.trim().parse().ok()
}

/// Packages keep the CLI in bin/ (Windows) or beside the desktop binary.
pub(crate) fn cli_path() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let directory = executable.parent().context("Missing Shellfox executable directory")?;
    let name = if cfg!(windows) { "shellfox-cli.exe" } else { "shellfox-cli" };
    [directory.join("bin").join(name), directory.join(name)]
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| format!("{name} is missing beside {}", executable.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(windows)]
mod platform {
    use std::{ffi::OsString, os::windows::process::CommandExt, path::Path, process::Command};
    // A hidden console of its own: its console children never flash a window,
    // and closing the terminal that started the UI does not end the host.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    /// Break away from a job the UI may run in, such as a launcher's kill-on-close job.
    pub fn scope(cli: &Path, arguments: &[OsString]) -> Option<Command> {
        let mut command = direct(cli, arguments);
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        Some(command)
    }
    pub fn direct(cli: &Path, arguments: &[OsString]) -> Command {
        let mut command = Command::new(cli);
        command.args(arguments).creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
        command
    }
}

#[cfg(unix)]
mod platform {
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
        command.args(scope_arguments(&uuid::Uuid::new_v4())).arg(cli).args(arguments);
        Some(command)
    }
    #[cfg(target_os = "linux")]
    pub(super) fn scope_arguments(unit: &uuid::Uuid) -> Vec<String> {
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

#[cfg(test)]
#[path = "host_process_tests.rs"]
mod tests;
