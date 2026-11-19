use crate::{external_cleanup::ExternalControl, *};
use std::time::Duration;
#[derive(Clone, Copy)]
struct StartupOptions {
    wait: Duration,
    display_policy: ExternalDisplayPolicy,
    #[cfg(feature = "experimental-shellfox-native")]
    native: Option<Native0RuntimeConfig>,
    #[cfg(test)]
    before_ready: Option<fn()>,
}
impl Default for StartupOptions {
    fn default() -> Self {
        Self {
            wait: Duration::from_secs(5),
            display_policy: ExternalDisplayPolicy::RawBytes,
            #[cfg(feature = "experimental-shellfox-native")]
            native: None,
            #[cfg(test)]
            before_ready: None,
        }
    }
}
impl Runtime {
    /// Starts the one passive parser before returning. No raw target may launch
    /// until Ok. A timeout/channel failure RETAINS a cancellable parser owner.
    pub fn start_external(
        identity: Identity,
        config: ExternalConfig,
    ) -> Result<Self, ExternalStartError> {
        start(identity, config, StartupOptions::default())
    }
    pub fn start_external_with_policy(
        identity: Identity,
        config: ExternalConfig,
        policy: ExternalDisplayPolicy,
    ) -> Result<Self, ExternalStartError> {
        start(
            identity,
            config,
            StartupOptions {
                display_policy: policy,
                ..Default::default()
            },
        )
    }
    pub fn start_external_with_hooks(
        identity: Identity,
        config: ExternalConfig,
        hooks: RuntimeHooks,
    ) -> Result<Self, ExternalStartError> {
        start_hooked(identity, config, StartupOptions::default(), hooks)
    }
    pub fn start_external_with_policy_and_hooks(
        identity: Identity,
        config: ExternalConfig,
        policy: ExternalDisplayPolicy,
        hooks: RuntimeHooks,
    ) -> Result<Self, ExternalStartError> {
        start_hooked(
            identity,
            config,
            StartupOptions {
                display_policy: policy,
                ..Default::default()
            },
            hooks,
        )
    }
    #[cfg(feature = "experimental-shellfox-native")]
    pub fn start_external_with_native0(
        identity: Identity,
        config: ExternalConfig,
        policy: ExternalDisplayPolicy,
        hooks: RuntimeHooks,
        native: Native0RuntimeConfig,
    ) -> Result<Self, ExternalStartError> {
        start_hooked(
            identity,
            config,
            StartupOptions {
                display_policy: policy,
                native: Some(native),
                ..Default::default()
            },
            hooks,
        )
    }
    /// Exactly one source exists; failed startup owners never expose this cap.
    pub fn take_source(&mut self) -> Option<ExternalSource> {
        self.external_source.take()
    }
    pub fn source_kind(&self) -> SourceKind {
        self.source_kind
    }
    /// Caller must independently prove raw cleanup. Source finish alone is not
    /// process proof; it only permits retiring this passive parser explicitly.
    pub fn request_external_close(&self) -> Result<(), RuntimeError> {
        let status = self
            .external_status()
            .ok_or_else(|| RuntimeError::validation("Not an external source."))?;
        if !status.finished && !status.failed {
            return Err(RuntimeError::validation("External source is not finished."));
        }
        self.shared.abandoned.store(true, Ordering::Release);
        Ok(())
    }
    pub fn poll_external_closed(&mut self) -> Result<bool, RuntimeError> {
        if self.source_kind != SourceKind::ExternalBytes {
            return Err(RuntimeError::validation("Not an external source."));
        }
        Ok(self
            .external_control
            .as_mut()
            .is_none_or(ExternalControl::poll_closed))
    }
    pub fn external_status(&self) -> Option<ExternalStatus> {
        self.external_status
            .as_ref()
            .map(|status| *status.lock().unwrap_or_else(|error| error.into_inner()))
    }
}
fn start(
    identity: Identity,
    config: ExternalConfig,
    options: StartupOptions,
) -> Result<Runtime, ExternalStartError> {
    start_hooked(identity, config, options, RuntimeHooks::default())
}
fn start_hooked(
    identity: Identity,
    config: ExternalConfig,
    options: StartupOptions,
    hooks: RuntimeHooks,
) -> Result<Runtime, ExternalStartError> {
    config
        .limits
        .validate_size(config.cols, config.rows)
        .map_err(|error| {
            ExternalStartError::before(std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
        })?;
    // Prestarted before parser creation, so even ignored/dropped startup errors
    // transfer the original JoinHandle to a cleanup owner instead of detaching it.
    let (source, source_owner) = external::channel_with_policy(identity, options.display_policy);
    let status = Arc::clone(&source_owner.status);
    let launch = Arc::clone(&source_owner.launch);
    let cleanup =
        ExternalControl::prepare(Arc::clone(&status)).map_err(ExternalStartError::before)?;
    let (commands, incoming) = mpsc::sync_channel(COMMAND_CAPACITY);
    let (outgoing, events) = mpsc::sync_channel(EVENT_CAPACITY);
    let (ready, started) = mpsc::sync_channel(1);
    let shared = Arc::new(Shared {
        screen: Mutex::new(None),
        legacy_requested: AtomicBool::new(false),
        live_output_status: AtomicU8::new(hooks.status() as u8),
        metadata: Mutex::new(None),
        phase: Mutex::new(Phase::Starting),
        abandoned: AtomicBool::new(false),
    });
    let owner_shared = Arc::clone(&shared);
    let thread = std::thread::Builder::new()
        .name("shellfox-external-view".into())
        .spawn(move || {
            #[cfg(test)]
            if let Some(before_ready) = options.before_ready {
                before_ready();
            }
            owner::run_external(
                identity,
                config,
                incoming,
                outgoing,
                owner_shared,
                source_owner,
                owner::ExternalStartServices {
                    ready,
                    hooks,
                    #[cfg(feature = "experimental-shellfox-native")]
                    native: options.native,
                },
            )
        })
        .map_err(ExternalStartError::before)?;
    let runtime = Runtime {
        identity,
        launch,
        limits: config.limits,
        commands,
        events,
        shared,
        inspection: None,
        source_kind: SourceKind::ExternalBytes,
        external_source: Some(source),
        external_status: Some(status),
        external_control: Some(cleanup.bind(thread)),
    };
    match started.recv_timeout(options.wait) {
        Ok(Ok(())) => Ok(runtime),
        Ok(Err(error)) => Err(ExternalStartError::retained(
            std::io::Error::other(error),
            runtime,
        )),
        Err(error) => Err(ExternalStartError::retained(
            std::io::Error::new(
                if matches!(error, mpsc::RecvTimeoutError::Timeout) {
                    std::io::ErrorKind::TimedOut
                } else {
                    std::io::ErrorKind::BrokenPipe
                },
                "External parser readiness was not confirmed; owner retained.",
            ),
            runtime,
        )),
    }
}
#[cfg(test)]
#[path = "external_startup_tests.rs"]
mod tests;
