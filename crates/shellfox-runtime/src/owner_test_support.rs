use super::*;
use shellfox_core::{Generation, SessionId, TabId};
pub(super) fn owner() -> Owner {
    Owner {
        identity: Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        },
        limits: GridLimits::default(),
        terminal: Terminal::new(80, 24).unwrap(),
        #[cfg(feature = "experimental-shellfox-native")]
        native_creation: None,
        presentations: presentation::Presentations::new(),
        layouts: layout::Layouts::new(80, 24),
        pty: None,
        source_kind: SourceKind::OwnedPty,
        external: None,
        delivery: Delivery::new(),
        observers: ObserverHub::new(),
        observers_first: false,
        phase: Phase::Active,
        shared: Arc::new(Shared {
            screen: Mutex::new(None),
            legacy_requested: AtomicBool::new(false),
            live_output_status: AtomicU8::new(LiveOutputStatus::Disabled as u8),
            metadata: Mutex::new(None),
            phase: Mutex::new(Phase::Active),
            abandoned: AtomicBool::new(false),
        }),
        outgoing: VecDeque::new(),
        dirty: false,
        fatal: false,
        exit_sent: false,
        retry_at: Instant::now(),
        inspection: None,
        live_output: hooks::LiveOutput::new(RuntimeHooks::default()),
    }
}
