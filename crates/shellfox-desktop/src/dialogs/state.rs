use crate::field::Field;
use gpui::Entity;
use shellfox_host::{SessionId, TabId};

/// The window's one modal dialog or in-place editor.
pub enum Dialog {
    Create(Box<super::directory::Draft>),
    RenameSession { id: SessionId, title: Entity<Field> },
    SessionEnvironment { id: SessionId, text: Entity<Field> },
    PeerSession(crate::peers::session_dialog::PeerSessionDialog),
    RenameTab { id: TabId, title: Entity<Field>, _blur: gpui::Subscription },
    DeleteSession(SessionId),
    CloseTab(shellfox_core::TerminalIdentity),
    Quit,
}

/// The modal dialog, if any, and whether a user action it started is still running.
pub(crate) struct DialogState {
    pub(crate) current: Option<Dialog>,
    pub(crate) modal_focus: gpui::FocusHandle,
    pub(crate) user_action_pending: bool,
}

impl DialogState {
    pub(crate) fn new(cx: &mut gpui::App) -> Self {
        Self { current: None, modal_focus: cx.focus_handle(), user_action_pending: false }
    }
}
