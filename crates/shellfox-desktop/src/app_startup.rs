//! Pre-GPU host connection. The UI attaches to a separate host process and
//! starts it when none answers; see host_process.rs.
use crate::{
    app_host::AppHost,
    host_client::HostEndpointConfig,
    options::{Mode, Options},
};
use anyhow::{Context, Result};
use shellfox_service::{
    desktop_launch::{LaunchRequestKind, LaunchRouteInfo},
    local::{LocalClient, OtherBuildRunning},
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct InitErrors(pub Rc<RefCell<Option<anyhow::Error>>>);
impl gpui::Global for InitErrors {}
pub(crate) fn record_init_failure(error: anyhow::Error, cx: &mut gpui::App) {
    if cx.has_global::<InitErrors>() {
        let mut first = cx.global::<InitErrors>().0.borrow_mut();
        if first.is_none() {
            *first = Some(error);
        }
    }
}

pub(crate) enum Startup {
    Forwarded,
    /// The user chose Quit over stopping the previous version's host.
    Declined,
    Graphical(Option<Prestarted>),
}
#[derive(Clone)]
pub(crate) struct Prestarted {
    pub config: HostEndpointConfig,
    pub host: AppHost,
}

pub(crate) fn prepare(options: &Options) -> Result<Startup> {
    if options.mode != Mode::Shell {
        return Ok(Startup::Graphical(None));
    }
    crate::frontend_composition::inspect(options)?;
    let config = options
        .endpoint
        .clone()
        .context("Trusted helper configuration is required before Host/UI startup")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(shellfox_service::local::check_helper_protocol(
        &config.helper,
        config.helper_sha256,
    ))?;
    // Released once this launch's window registers, or it forwards or quits.
    crate::launch_lock::acquire(&config.data_dir);
    let running = crate::host_process::find_running(&runtime, &config)?;
    let running = match running {
        Some(running) => match stale_host(&runtime, options, &running)? {
            // A previous build's host was stopped; start the right one.
            StaleOutcome::Replaced => None,
            StaleOutcome::Attach => Some(running),
            StaleOutcome::Declined => {
                crate::launch_lock::release();
                return Ok(Startup::Declined);
            }
        },
        None => None,
    };
    let Some(running) = running else {
        // The host boots while the UI initializes; windows wait for it.
        let route = crate::host_process::start_in_background(
            options.relaunch_args.clone(),
            options.application_args.clone(),
            config.clone(),
        )?;
        let host = AppHost::attached(config.clone(), route, true)?;
        return Ok(Startup::Graphical(Some(Prestarted { config, host })));
    };
    let route = running.route;
    if route.desktop_registered {
        // Another window process owns the UI: hand this launch to it.
        handoff(&runtime, &running.client, options, route)?;
        crate::launch_lock::release();
        return Ok(Startup::Forwarded);
    }
    let _ = runtime.block_on(running.client.disconnect());
    let owned = options.owned_host || recorded(options, route);
    let route = crate::host_process::Route::known(crate::host_process::HostRoute {
        info: route,
        started: false,
    });
    let host = AppHost::attached(config.clone(), route, owned)?;
    Ok(Startup::Graphical(Some(Prestarted { config, host })))
}

#[derive(Debug, PartialEq, Eq)]
enum Stale {
    Attach,
    Replace,
    Ask,
}
/// Only a desktop-started host of this exact build is attached. An idle one of
/// another build is replaced. One with shells or a window is replaced only if
/// the user agrees: its shells are unreachable from this build, so leaving it
/// would keep them running unseen.
fn stale(
    desktop_owned: bool,
    same_build: bool,
    desktop_registered: bool,
    terminals: usize,
) -> Stale {
    match (desktop_owned, same_build) {
        (false, _) | (true, true) => Stale::Attach,
        (true, false) if !desktop_registered && terminals == 0 => Stale::Replace,
        (true, false) => Stale::Ask,
    }
}

enum StaleOutcome {
    Attach,
    Replaced,
    Declined,
}
fn stale_question(terminals: usize) -> (String, &'static str) {
    match terminals {
        0 => (
            "Shellfox was updated. The previous version is still open.".into(),
            "Close it and continue",
        ),
        1 => (
            "Shellfox was updated. 1 terminal is still running in the previous version.".into(),
            "Stop it and continue",
        ),
        n => (
            format!(
                "Shellfox was updated. {n} terminals are still running in the previous version."
            ),
            "Stop them and continue",
        ),
    }
}

fn stale_host(
    runtime: &tokio::runtime::Runtime,
    options: &Options,
    running: &crate::host_process::Running,
) -> Result<StaleOutcome> {
    let route = running.route;
    let desktop_owned = options.owned_host || recorded(options, route);
    let same_build = route.build == crate::host_process::expected_build();
    let terminals = if desktop_owned && !same_build {
        runtime.block_on(terminal_count(&running.client))?
    } else {
        0
    };
    match stale(
        desktop_owned,
        same_build,
        route.desktop_registered,
        terminals,
    ) {
        Stale::Attach => Ok(StaleOutcome::Attach),
        Stale::Ask if options.ui_acceptance => Err(OtherBuildRunning.into()),
        Stale::Ask => {
            let (question, accept) = stale_question(terminals);
            match crate::native_dialog::confirm(&question, accept, "Quit") {
                Some(true) => replace(runtime, running),
                Some(false) => Ok(StaleOutcome::Declined),
                None => Err(OtherBuildRunning.into()),
            }
        }
        Stale::Replace => replace(runtime, running),
    }
}

/// Stops another build's host, ending its shells, and waits until it is gone.
fn replace(
    runtime: &tokio::runtime::Runtime,
    running: &crate::host_process::Running,
) -> Result<StaleOutcome> {
    runtime.block_on(async {
        running
            .client
            .execute(shellfox_service::local_api::LocalApiRequest::Stop {})
            .await?;
        tokio::time::timeout(Duration::from_secs(30), running.client.closed())
            .await
            .context("The previous Shellfox host did not stop")?;
        anyhow::Ok(())
    })?;
    Ok(StaleOutcome::Replaced)
}

async fn terminal_count(client: &LocalClient) -> Result<usize> {
    match client
        .request(&shellfox_service::local::DesktopRequest::Status)
        .await?
    {
        shellfox_service::local::DesktopReply::Status(status) => Ok(status.active_terminals),
        _ => anyhow::bail!("Host returned an invalid status response."),
    }
}

fn recorded(options: &Options, route: LaunchRouteInfo) -> bool {
    crate::host_process::recorded_owner(&options.desktop_dir) == Some(route.owner)
}

fn handoff(
    runtime: &tokio::runtime::Runtime,
    client: &LocalClient,
    options: &Options,
    route: LaunchRouteInfo,
) -> Result<()> {
    runtime
        .block_on(client.launch_existing_pinned(
            options.launch.clone(),
            LaunchRequestKind::Fresh,
            route.owner,
        ))
        .context("The running Shellfox did not accept this launch")?;
    Ok(())
}

pub(crate) enum Ready {
    Attach(Option<Box<crate::startup_chrome::StartupChrome>>),
    /// A concurrent launch's UI owns the host it started.
    Forwarded,
}
/// Waits for the host before any window opens, then reads the window chrome.
pub(crate) fn ready(
    host: &AppHost,
    config: &HostEndpointConfig,
    options: &Options,
) -> Result<Ready> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let route = runtime.block_on(host.route())?;
    if !route.started {
        // A concurrent launch's host answered instead of ours: it is ours only
        // if it is the one this UI is meant to own.
        host.set_owned(options.owned_host || recorded(options, route.info));
        if route.info.build != crate::host_process::expected_build() && !host.is_external() {
            host.host_ended();
            return Err(OtherBuildRunning.into());
        }
        if route.info.desktop_registered {
            let client = runtime.block_on(LocalClient::connect(
                &config.data_dir,
                &config.helper,
                config.helper_sha256,
            ))?;
            handoff(&runtime, &client, options, route.info)?;
            let _ = runtime.block_on(client.disconnect());
            host.host_ended();
            return Ok(Ready::Forwarded);
        }
    }
    if options.ui_acceptance {
        return Ok(Ready::Attach(None));
    }
    let chrome = runtime.block_on(async {
        anyhow::Ok(crate::startup_chrome::StartupChrome::read(host.chrome_client().await?).await)
    });
    Ok(Ready::Attach(
        chrome
            .inspect_err(|error| eprintln!("Startup chrome unavailable: {error:#}"))
            .ok()
            .map(Box::new),
    ))
}
impl Prestarted {
    /// Called once as the app ends (app_exit.rs), including after a GPU failure.
    /// A UI that ends without a confirmed Quit leaves live shells to their host,
    /// as a crash would; an idle desktop-started host is stopped.
    pub fn cleanup_until_confirmed(&self) {
        if self.host.is_stopped().unwrap_or(false) || self.host.is_external() {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let result = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(async {
                    match self.host.owned_terminal_count().await {
                        Ok(0) => self.host.cleanup().await,
                        Ok(_) => Ok(()),
                        Err(error) => Err(error),
                    }
                }),
                Err(error) => Err(error.into()),
            };
            let Err(error) = result else { return };
            if Instant::now() >= deadline {
                eprintln!("Host cleanup is unconfirmed; leaving the host running: {error:#}");
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_desktop_host_of_this_build_is_attached() {
        assert_eq!(stale(true, true, true, 3), Stale::Attach);
        assert_eq!(
            stale(false, false, false, 3),
            Stale::Attach,
            "a `host run` host"
        );
        assert_eq!(stale(true, false, false, 0), Stale::Replace);
        assert_eq!(stale(true, false, false, 1), Stale::Ask);
        assert_eq!(stale(true, false, true, 0), Stale::Ask);
    }
    #[test]
    fn the_stale_question_counts_the_terminals() {
        assert_eq!(
            stale_question(2),
            (
                "Shellfox was updated. 2 terminals are still running in the previous version."
                    .into(),
                "Stop them and continue"
            )
        );
        assert_eq!(stale_question(1).1, "Stop it and continue");
        assert_eq!(stale_question(0).1, "Close it and continue");
    }
}
