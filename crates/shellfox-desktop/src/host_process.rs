//! The UI is a client of a separate host process, `shellfox-cli --desktop-host`
//! (see shellfox-cli/src/desktop_host.rs), so a UI crash leaves every shell
//! running. The host relaunches a crashed UI once; later launches reattach.
use crate::host_client::HostEndpointConfig;
use anyhow::{Context, Result, bail};
use shellfox_service::{
    desktop_launch::{DesktopLaunchReply, DesktopLaunchRequest, LaunchOwnerFence, LaunchRouteInfo},
    local::{LocalBootstrap, LocalClient, OtherBuildRunning},
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
use tokio::sync::watch;

/// The host this UI started. Its stdin stays open for this process's life.
static STARTED: Mutex<Option<Started>> = Mutex::new(None);
struct Started {
    stdin: ChildStdin,
    child: std::process::Child,
}
const READY_TIMEOUT: Duration = Duration::from_secs(60);
static QUITTING: AtomicBool = AtomicBool::new(false);
const OWNER_MARKER: &str = "desktop-host-owner";

pub(crate) struct Running {
    pub client: LocalClient,
    pub route: LaunchRouteInfo,
}

/// The host as the startup thread found it once it answered.
#[derive(Clone, Copy)]
pub(crate) struct HostRoute {
    pub info: LaunchRouteInfo,
    /// This UI started the host, so its Quit stops it.
    pub started: bool,
}
/// Becomes known once the host answers. Windows wait on it before attaching.
#[derive(Clone)]
pub(crate) struct Route(watch::Receiver<Option<Result<HostRoute, String>>>);
impl Route {
    pub fn known(route: HostRoute) -> Self {
        Self(watch::channel(Some(Ok(route))).1)
    }
    pub async fn get(&self) -> Result<HostRoute> {
        let mut route = self.0.clone();
        let known = route
            .wait_for(Option::is_some)
            .await
            .map_err(|_| anyhow::anyhow!("Host startup ended."))?;
        known
            .clone()
            .context("Host route missing")?
            .map_err(anyhow::Error::msg)
    }
}

/// The data directory's host, if one answers now.
/// None when no host answers. A live host of another build is an error.
pub(crate) fn find_running(
    runtime: &tokio::runtime::Runtime,
    config: &HostEndpointConfig,
) -> Result<Option<Running>> {
    match runtime.block_on(discover(config)) {
        Ok((client, route)) => Ok(Some(Running { client, route })),
        Err(error) if error.is::<OtherBuildRunning>() => Err(error),
        Err(_) => Ok(None),
    }
}

/// Starts a host on its own thread, so the UI can initialize meanwhile. A host
/// started by a concurrent launch wins the directory lock; ours then exits and
/// the route names the winner.
pub(crate) fn start_in_background(
    relaunch_args: Vec<OsString>,
    application_args: Vec<OsString>,
    config: HostEndpointConfig,
) -> Result<Route> {
    let (sender, route) = watch::channel(None);
    std::thread::Builder::new()
        .name("shellfox-host-start".into())
        .spawn(move || {
            let found = (|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let executable = std::env::current_exe()?;
                let (client, info, started) = start_and_discover(
                    &runtime,
                    &executable,
                    &relaunch_args,
                    &application_args,
                    &config,
                )?;
                let _ = runtime.block_on(client.disconnect());
                anyhow::Ok(HostRoute { info, started })
            })();
            let _ = sender.send(Some(found.map_err(|error| format!("{error:#}"))));
        })?;
    Ok(Route(route))
}

fn start_and_discover(
    runtime: &tokio::runtime::Runtime,
    executable: &Path,
    relaunch_args: &[OsString],
    application_args: &[OsString],
    config: &HostEndpointConfig,
) -> Result<(LocalClient, LaunchRouteInfo, bool)> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        // A host that lost the directory lock exits silently: a concurrent
        // launch's host answers soon, or a stopping one releases the lock.
        let (started, patience) = match start(executable, relaunch_args, application_args, config) {
            Ok(()) => (true, deadline),
            Err(StartError::Permanent(error)) => return Err(error),
            Err(StartError::Exited) => (
                false,
                (Instant::now() + Duration::from_secs(3)).min(deadline),
            ),
        };
        loop {
            match runtime.block_on(discover(config)) {
                Ok((client, route)) => return Ok((client, route, started)),
                Err(error) if Instant::now() >= deadline => {
                    if started {
                        // Never leave a host that this UI cannot reach.
                        stop_started(Duration::from_secs(10));
                    }
                    return Err(error.context("The Shellfox host did not answer"));
                }
                Err(_) if Instant::now() >= patience => break,
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }
}

/// Whether a host still answers on this data directory, asked a few times.
pub(crate) async fn serving(config: &HostEndpointConfig) -> bool {
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        if let Ok((client, _)) = discover(config).await {
            let _ = client.disconnect().await;
            return true;
        }
    }
    false
}

async fn discover(config: &HostEndpointConfig) -> Result<(LocalClient, LaunchRouteInfo)> {
    let client =
        LocalClient::connect(&config.data_dir, &config.helper, config.helper_sha256).await?;
    let DesktopLaunchReply::Route(route) =
        client.request(&DesktopLaunchRequest::Discover {}).await?
    else {
        bail!("The host returned an invalid desktop discovery response.");
    };
    Ok((client, route))
}

enum StartError {
    /// Retrying cannot help: no CLI, it cannot be spawned, or it is not a host.
    Permanent(anyhow::Error),
    /// The host exited silently, as one does after losing the directory lock
    /// to a concurrent launch's host. Discovery finds the winner.
    Exited,
}

/// Starts the host and waits until its local API admits clients.
fn start(
    executable: &Path,
    relaunch_args: &[OsString],
    application_args: &[OsString],
    config: &HostEndpointConfig,
) -> Result<(), StartError> {
    let cli = cli_beside(executable).map_err(StartError::Permanent)?;
    let mut arguments = vec![
        LocalBootstrap::DESKTOP_HOST_FLAG.into(),
        executable.as_os_str().to_owned(),
    ];
    arguments.push(relaunch_args.len().to_string().into());
    arguments.extend(relaunch_args.iter().cloned());
    arguments.extend([
        "--data-dir".into(),
        config.data_dir.clone().into_os_string(),
    ]);
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
        None => spawn(&mut platform::direct(&cli, &arguments))
            .map_err(StartError::Permanent)?
            .ok_or(StartError::Exited)?,
    };
    if output == LocalBootstrap::DESKTOP_HOST_READY {
        return Ok(());
    }
    Err(StartError::Permanent(anyhow::anyhow!(
        "The Shellfox host did not start: {output}"
    )))
}

/// Returns the host's first stdout line, or None if it printed nothing.
fn spawn(command: &mut Command) -> Result<Option<String>> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Released hosts outlive this UI and any terminal it was started from.
        .stderr(if cfg!(debug_assertions) {
            Stdio::inherit()
        } else {
            Stdio::null()
        });
    let mut child = command.spawn().context("Start the Shellfox host")?;
    let stdout = child.stdout.take().context("Host output missing")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = sender.send(BufReader::new(stdout).read_line(&mut line).map(|_| line));
    });
    let line = match receiver.recv_timeout(READY_TIMEOUT) {
        Ok(line) => line?.trim_end().to_owned(),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            bail!("The Shellfox host did not become ready in time");
        }
    };
    if line == LocalBootstrap::DESKTOP_HOST_READY {
        let stdin = child.stdin.take().context("Host input missing")?;
        // Not waited here: the host outlives this UI by design.
        *STARTED.lock().unwrap_or_else(|error| error.into_inner()) = Some(Started { stdin, child });
        return Ok(Some(line));
    }
    let _ = child.wait();
    Ok((!line.is_empty()).then_some(line))
}

/// Call once this UI's Stop was admitted, so the host does not take the UI's
/// exit for a crash and relaunch it.
pub(crate) fn announce_quit() {
    QUITTING.store(true, Ordering::SeqCst);
    if let Some(started) = STARTED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_mut()
    {
        let _ = started
            .stdin
            .write_all(&[LocalBootstrap::DESKTOP_HOST_QUIT]);
        let _ = started.stdin.flush();
    }
}

/// This UI is stopping its host, so the host's end is expected.
pub(crate) fn quit_announced() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Whether this UI started the host process it is attached to.
pub(crate) fn started_host() -> bool {
    STARTED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .is_some()
}

/// Waits for the host this UI started to exit. False if it is still running,
/// or if this UI did not start it.
pub(crate) fn wait_started_exit(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let exited = match STARTED
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_mut()
        {
            Some(started) => !matches!(started.child.try_wait(), Ok(None)),
            None => return false,
        };
        if exited {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Ends a host this UI started but could not reach: no shell can be in it.
fn stop_started(grace: Duration) {
    announce_quit();
    if !wait_started_exit(grace) {
        kill_started();
    }
}

/// Ends a host this UI started that stopped answering. No-op for a host this
/// UI didn't start.
pub(crate) fn kill_started() {
    if let Some(started) = STARTED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_mut()
    {
        let _ = started.child.kill();
        let _ = started.child.wait();
    }
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
    std::fs::read_to_string(desktop_dir.join(OWNER_MARKER))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The build stamp a host from this build reports.
pub(crate) fn expected_build() -> u64 {
    shellfox_service::desktop_launch::build_stamp(env!("SHELLFOX_BUILD_ID"))
}

/// Packages keep the CLI in bin/ (Windows) or beside the desktop binary.
pub(crate) fn cli_path() -> Result<PathBuf> {
    cli_beside(&std::env::current_exe()?)
}
fn cli_beside(executable: &Path) -> Result<PathBuf> {
    let directory = executable
        .parent()
        .context("Missing Shellfox executable directory")?;
    let name = if cfg!(windows) {
        "shellfox-cli.exe"
    } else {
        "shellfox-cli"
    };
    [directory.join("bin").join(name), directory.join(name)]
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| format!("{name} is missing beside {}", executable.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

mod platform;

#[cfg(test)]
mod tests;
