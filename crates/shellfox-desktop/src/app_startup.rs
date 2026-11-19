//! Pre-GPU ownership. Every acquired endpoint is retained until its actual Stop is confirmed.
use crate::{
    app_host::{AppHost, AppHostPrestart},
    host_client::HostEndpointConfig,
    options::{Mode, Options},
};
use anyhow::{Context, Result};
use shellfox_service::{
    desktop_launch::{DesktopLaunchReply, DesktopLaunchRequest, LaunchRequestKind}, local::LocalClient, remote::RemoteUnavailableReason,
};
use std::{
    cell::RefCell,
    fmt,
    rc::Rc,
    sync::Arc,
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
    pub remote_unavailable: Option<RemoteUnavailableReason>,
}
struct StartupFailure(Arc<anyhow::Error>);
impl fmt::Debug for StartupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StartupFailure(..)")
    }
}
impl fmt::Display for StartupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for StartupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref().as_ref())
    }
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
    let remote = options
        .application
        .remote_options(std::env::current_exe()?, true)?;
    match runtime.block_on(AppHost::prestart_remote(config.clone(), remote))? {
        AppHostPrestart::Ready(host) => Ok(Startup::Graphical(Some(Prestarted {
            config,
            host,
            remote_unavailable: None,
        }))),
        AppHostPrestart::ReadyLocalOnly {
            host,
            remote_unavailable,
        } => Ok(Startup::Graphical(Some(Prestarted {
            config,
            host,
            remote_unavailable: Some(remote_unavailable),
        }))),
        AppHostPrestart::Occupied(proof) => {
            // The sealed acquire(None)+contender-exit proof authorizes this attempt,
            // not a particular future owner fence. Discovery/receipts capture that once.
            let attach = runtime.block_on(async {
                let client = LocalClient::connect(proof.directory(), &config.helper, config.helper_sha256).await?;
                let DesktopLaunchReply::Route(route) = client.request(&DesktopLaunchRequest::Discover {}).await? else {
                    anyhow::bail!("Existing host returned an invalid desktop discovery response.");
                };
                if route.desktop_registered {
                    client.launch_existing_pinned(options.launch.clone(), LaunchRequestKind::Fresh, route.owner).await?;
                    Ok(None)
                } else {
                    Ok::<_, anyhow::Error>(Some(route.owner))
                }
            }).context("Existing host could not be contacted; another host was not started")?;
            if let Some(owner) = attach {
                let host = AppHost::attached(config.clone(), owner)?;
                Ok(Startup::Graphical(Some(Prestarted { config, host, remote_unavailable: None })))
            } else {
                Ok(Startup::Forwarded)
            }
        }
        AppHostPrestart::Failed { host, error } => {
            let retained = Prestarted {
                config,
                host,
                remote_unavailable: None,
            };
            retained.cleanup_until_confirmed();
            Err(anyhow::Error::new(StartupFailure(error)))
        }
    }
}
impl Prestarted {
    /// Called outside Application.run, including after installation/GPU failure.
    /// No timeout/error authorizes returning while the original owner is unconfirmed.
    pub fn cleanup_until_confirmed(&self) {
        if self.host.is_stopped().unwrap_or(false) {
            return;
        }
        let mut next_diagnostic = Instant::now();
        loop {
            let result = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(self.host.cleanup_prestarted()),
                Err(error) => Err(error.into()),
            };
            match result {
                Ok(outcome) => {
                    if !self.host.is_attached() { crate::root::log_shutdown_report(&mut std::io::stderr().lock(), Ok(outcome)); }
                    return;
                }
                Err(error) => {
                    if Instant::now() >= next_diagnostic {
                        eprintln!(
                            "Prestarted host cleanup is unconfirmed; retaining the exact owner and retrying: {error:#}"
                        );
                        next_diagnostic = Instant::now() + Duration::from_secs(30);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "app_startup_owned_tests.rs"]
mod owned_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug)]
    struct Marker;
    impl fmt::Display for Marker {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("typed startup marker")
        }
    }
    impl std::error::Error for Marker {}
    #[tokio::test(flavor = "current_thread")]
    async fn remote_composition_failure_retains_same_endpoint_without_materializing_node_state() {
        let path = std::env::current_dir().unwrap().join("tmp").join(format!(
            "app-remote-failure-{}",
            shellfox_core::SessionId::new()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let application = crate::options::app_composition::NativeApplicationConfig::new(
            &path,
            None,
            Some("owned-app-failure".into()),
            crate::options::app_composition::SourceFactory::Local,
        )
        .unwrap();
        let state = application.remote_state_dir.clone();
        let remote = application
            .remote_options(std::env::current_exe().unwrap(), false)
            .unwrap();
        let config = HostEndpointConfig {
            data_dir: path.clone(),
            helper: path.join("missing-helper.exe"),
            helper_sha256: [0; 32],
        };
        let AppHostPrestart::Failed { host, error } =
            AppHost::prestart_remote(config, remote).await.unwrap()
        else {
            panic!("exact retained remote startup failure required")
        };
        assert!(
            error
                .downcast_ref::<shellfox_service::local::OccupiedOwnerError>()
                .is_none()
        );
        let same = host.clone();
        assert!(same.same_owner(&host));
        host.cleanup_prestarted().await.unwrap();
        assert!(same.is_stopped().unwrap());
        assert!(
            !state.exists(),
            "Optional node state must not be caller-created before native admission"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn retained_startup_error_preserves_original_typed_source_without_debug_payload() {
        let original = Arc::new(anyhow::Error::new(Marker));
        let error = anyhow::Error::new(StartupFailure(original.clone()));
        assert!(error.chain().any(|source| source.is::<Marker>()));
        assert_eq!(Arc::strong_count(&original), 2);
        assert_eq!(
            format!("{:?}", StartupFailure(original)),
            "StartupFailure(..)"
        );
    }
}
