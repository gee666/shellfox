use shellfox_host::{SessionId, TabId};

/// A reply may change focus only while both the machine and its selection are current.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActivationFence {
    pub source: u64,
    pub navigation: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum RecentSelection {
    Session(SessionId),
    Tab(TabId),
}

/// Snapshot refreshes and selection writes run beside user actions, never
/// blocking them. Each kind keeps one request in flight; ordering comes from
/// snapshot revisions.
#[derive(Default)]
pub(crate) struct Background {
    pub(super) refresh: bool,
    pub(super) refresh_again: bool,
    pub(super) remember: bool,
}

/// Which session and tab the window shows, and the host requests that change them.
pub(crate) struct WorkspaceState {
    pub(crate) navigation: crate::window_manager::WindowSelection,
    pub(crate) pending_recent: Option<(RecentSelection, u64)>,
    pub(crate) activating_session: Option<(SessionId, ActivationFence)>,
    pub(crate) action_pending: bool,
    pub(crate) background: Background,
    /// Terminal profiles of the local host, for New terminal.
    pub(crate) profiles: Vec<crate::settings::api::Profile>,
    pub(crate) catalog_epoch: u64,
}

impl WorkspaceState {
    pub(crate) fn new(initial: crate::window_manager::InitialSelection) -> Self {
        Self {
            navigation: crate::window_manager::WindowSelection::new(initial),
            pending_recent: None,
            activating_session: None,
            action_pending: false,
            background: Background::default(),
            profiles: Vec::new(),
            catalog_epoch: 0,
        }
    }
}
