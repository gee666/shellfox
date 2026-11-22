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
        presentations: presentation::Presentations::new(),
        layouts: layout::Layouts::new(80, 24),
        pty: None,
        #[cfg(windows)]
        elevated: None,
        source_kind: SourceKind::OwnedPty,
        external: None,
        delivery: Delivery::new(),
        observers: ObserverHub::new(),
        observers_first: false,
        phase: Phase::Active,
        shared: Arc::new(Shared::new(LiveOutputStatus::Disabled, Phase::Active)),
        outgoing: VecDeque::new(),
        dirty: false,
        fatal: false,
        exit_sent: false,
        retry_at: Instant::now(),
        exit: None,
        inspection: None,
        live_output: hooks::LiveOutput::new(RuntimeHooks::default()),
    }
}
