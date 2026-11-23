use std::sync::Weak;

/// UI registry identity only. The service allocates the actual controller identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WindowId(pub(super) u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowCloseGuard {
    Ready,
    DirtyFile,
    FileOperation,
    HostOperation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseTarget {
    Window(WindowId),
    Quit,
}
#[derive(Clone, Debug)]
pub struct CloseTicket {
    pub(super) owner: Weak<()>,
    pub(super) revision: u64,
    pub(super) target: CloseTarget,
}
#[derive(Clone, Debug)]
pub struct CloseConfirmation {
    pub(super) ticket: CloseTicket,
    pub terminal_count: usize,
}
impl CloseConfirmation {
    /// Only the existing quit modal's affirmative response calls this.
    /// Dropping a confirmation is cancellation and changes nothing in the host.
    pub fn approve(self) -> CloseTicket {
        self.ticket
    }
}
#[derive(Clone, Debug)]
pub enum CloseDecision {
    Blocked(Vec<(WindowId, WindowCloseGuard)>),
    Confirm(CloseConfirmation),
    Ready(CloseTicket),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosedWindows {
    pub windows: Vec<WindowId>,
    pub app_stopped: bool,
}
