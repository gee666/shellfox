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
    local::LocalClient,
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
    Graphical(Option<Prestarted>),
}
pub(crate) struct Prestarted {
    pub config: HostEndpointConfig,
    pub host: AppHost,
}

pub(crate) fn prepare(options: &Options) -> Result<Startup> {
    if options.mode != Mode::Shell {
        return Ok(Startup::Graphical(None));
    }
    // Internal flags remain pinned and never try ownership acquisition.
    if crate::launch_handoff::forward_existing(options)? {
        return Ok(Startup::Forwarded);
    }
    let _frontend_proofs = crate::frontend_composition::inspect(options)?;
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
    let Some(running) = crate::host_process::find_running(&runtime, &config) else {
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
        return Ok(Startup::Forwarded);
    }
    let _ = runtime.block_on(running.client.disconnect());
    let owned = options.owned_host || recorded(options, route);
    let route = crate::host_process::Route::known(crate::host_process::HostRoute { info: route, started: false });
    let host = AppHost::attached(config.clone(), route, owned)?;
    Ok(Startup::Graphical(Some(Prestarted { config, host })))
}

fn recorded(options: &Options, route: LaunchRouteInfo) -> bool {
    crate::host_process::recorded_owner(&options.desktop_dir) == Some(route.owner)
}

fn handoff(runtime: &tokio::runtime::Runtime, client: &LocalClient, options: &Options, route: LaunchRouteInfo) -> Result<()> {
    runtime
        .block_on(client.launch_existing_pinned(options.launch.clone(), LaunchRequestKind::Fresh, route.owner))
        .context("The running Shellfox did not accept this launch")?;
    Ok(())
}

pub(crate) enum Ready {
    Attach(Option<crate::startup_chrome::StartupChrome>),
    /// A concurrent launch's UI owns the host it started.
    Forwarded,
}
/// Waits for the host before any window opens, then reads the window chrome.
/// Records a desktop-started host so a reattached UI stops it on Quit.
pub(crate) fn ready(host: &AppHost, config: &HostEndpointConfig, options: &Options) -> Result<Ready> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let route = runtime.block_on(host.route())?;
    if !route.started {
        if route.info.desktop_registered {
            let client = runtime.block_on(LocalClient::connect(&config.data_dir, &config.helper, config.helper_sha256))?;
            handoff(&runtime, &client, options, route.info)?;
            let _ = runtime.block_on(client.disconnect());
            host.host_ended();
            return Ok(Ready::Forwarded);
        }
        host.set_owned(options.owned_host || recorded(options, route.info));
    }
    if !host.is_external()
        && let Err(error) = crate::host_process::record_owner(&options.desktop_dir, route.info.owner)
    {
        eprintln!("Shellfox host ownership not recorded: {error:#}");
    }
    if options.ui_acceptance {
        return Ok(Ready::Attach(None));
    }
    let chrome = runtime.block_on(async {
        anyhow::Ok(crate::startup_chrome::StartupChrome::read(host.chrome_client().await?).await)
    });
    Ok(Ready::Attach(chrome.inspect_err(|error| eprintln!("Startup chrome unavailable: {error:#}")).ok()))
}
impl Prestarted {
    /// Called outside Application.run, including after installation/GPU failure.
    /// A UI that ends without a confirmed Quit leaves live shells to their host,
    /// as a crash would; an idle desktop-started host is stopped.
    pub fn cleanup_until_confirmed(&self) {
        if self.host.is_stopped().unwrap_or(false) || self.host.is_external() {
            return;
        }
        let mut next_diagnostic = Instant::now();
        loop {
            let result = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(async {
                    match self.host.owned_terminal_count().await {
                        Ok(0) => self.host.cleanup().await,
                        Ok(_) => Ok(()),
                        Err(_) if !crate::host_process::serving(&self.config) => Ok(()),
                        Err(error) => Err(error),
                    }
                }),
                Err(error) => Err(error.into()),
            };
            match result {
                Ok(()) => return,
                Err(error) => {
                    if Instant::now() >= next_diagnostic {
                        eprintln!("Host cleanup is unconfirmed; retrying: {error:#}");
                        next_diagnostic = Instant::now() + Duration::from_secs(30);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }
    }
}
