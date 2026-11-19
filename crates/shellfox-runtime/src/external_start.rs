use super::*;
use crate::external::{ExternalConfig, ExternalOwner};
pub(crate) struct ExternalStartServices {
    pub ready: SyncSender<Result<(), RuntimeError>>,
    pub hooks: RuntimeHooks,
    #[cfg(feature = "experimental-shellfox-native")]
    pub native: Option<Native0RuntimeConfig>,
}
pub(crate) fn run(
    identity: Identity,
    config: ExternalConfig,
    incoming: Receiver<Request>,
    events: SyncSender<Event>,
    shared: Arc<Shared>,
    source: ExternalOwner,
    services: ExternalStartServices,
) {
    let ready = services.ready;
    let create = || Terminal::new(config.cols, config.rows).map_err(RuntimeError::unavailable);
    #[cfg(feature = "experimental-shellfox-native")]
    let created = match services.native {
        Some(native) => crate::recovery::Native0Creation::create(config.cols, config.rows, native)
            .map(|(terminal, native)| (terminal, Some(native))),
        None => create().map(|terminal| (terminal, None)),
    };
    #[cfg(not(feature = "experimental-shellfox-native"))]
    let created = create();
    let created = match created {
        Ok(created) => created,
        Err(error) => {
            *shared
                .phase
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Phase::Closed;
            let _ = ready.send(Err(error.clone()));
            let _ = events.try_send(Event::Started(Err(error)));
            return;
        }
    };
    #[cfg(feature = "experimental-shellfox-native")]
    let (terminal, native_creation) = created;
    #[cfg(not(feature = "experimental-shellfox-native"))]
    let terminal = created;
    let mut owner = Owner {
        identity,
        limits: config.limits,
        terminal,
        #[cfg(feature = "experimental-shellfox-native")]
        native_creation,
        presentations: presentation::Presentations::new(),
        layouts: layout::Layouts::new(config.cols, config.rows),
        pty: None,
        #[cfg(windows)]
        elevated: None,
        source_kind: SourceKind::ExternalBytes,
        external: Some(source),
        delivery: Delivery::new(),
        observers: ObserverHub::new(),
        observers_first: false,
        phase: Phase::Starting,
        shared,
        outgoing: VecDeque::new(),
        dirty: true,
        fatal: false,
        exit_sent: false,
        retry_at: Instant::now(),
        inspection: None,
        live_output: hooks::LiveOutput::new(services.hooks),
    };
    if owner.shared.abandoned.load(Ordering::Acquire) {
        let _ = owner.cleanup();
        let _ = ready.send(Err(RuntimeError::unavailable(
            "External view start was cancelled.",
        )));
        return;
    }
    owner.set_phase(Phase::Active, None);
    owner.outgoing.push_front(Event::Started(Ok(())));
    if ready.send(Ok(())).is_err() {
        let _ = owner.cleanup();
        return;
    }
    drive(owner, incoming, events);
}
