//! Pre-GPU host connection. The UI attaches to a separate host process and
//! starts it when none answers; see host_process.rs.
use crate::{
    app_host::AppHost,
    host_client::HostEndpointConfig,
    options::{Mode, Options},
};
use anyhow::{Context, Result};
use shellfox_service::desktop_launch::{LaunchOwnerFence, LaunchRequestKind};
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
    /// The desktop-started host this UI stops on Quit, recorded once the
    /// desktop directory exists.
    pub owned_by: Option<LaunchOwnerFence>,
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
    let connected = crate::host_process::connect_or_start(&runtime, &options.relaunch_args, &options.application_args, &config)?;
    let route = connected.route;
    if route.desktop_registered {
        // Another window process owns the UI: hand this launch to it.
        runtime
            .block_on(connected.client.launch_existing_pinned(options.launch.clone(), LaunchRequestKind::Fresh, route.owner))
            .context("The running Shellfox did not accept this launch")?;
        return Ok(Startup::Forwarded);
    }
    let _ = runtime.block_on(connected.client.disconnect());
    let owned = connected.started
        || options.owned_host
        || crate::host_process::recorded_owner(&options.desktop_dir) == Some(route.owner);
    let host = AppHost::attached(config.clone(), route.owner, owned)?;
    Ok(Startup::Graphical(Some(Prestarted {
        config,
        host,
        owned_by: owned.then_some(route.owner),
    })))
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
