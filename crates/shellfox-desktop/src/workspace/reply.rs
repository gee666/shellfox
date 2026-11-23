//! Focus only the actual operation reply, correlated by original metadata receipt.
use super::ActivationFence;
use shellfox_host::{CreationRequestReceipt, SessionId, Snapshot, TabId, TabState};

pub(super) fn activation_reopens_history(
    requested: (SessionId, ActivationFence),
    selected: Option<SessionId>,
    current: ActivationFence,
    reply: &Snapshot,
) -> bool {
    let (session_id, fence) = requested;
    current == fence
        && selected == Some(session_id)
        && reply.sessions.iter().any(|session| {
            session.id == session_id
                && session
                    .tabs
                    .iter()
                    .all(|tab| tab.state == TabState::Archived)
        })
}
#[derive(Clone, Copy)]
pub(super) enum ReplyIntent {
    None,
    Create,
    AddTab { session_id: SessionId },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReplyFocus {
    pub session: SessionId,
    pub tab: Option<TabId>,
}
pub(super) fn focus_target(
    intent: ReplyIntent,
    receipt: Option<CreationRequestReceipt>,
    reply: &Snapshot,
    epoch_current: bool,
) -> Option<ReplyFocus> {
    if !epoch_current {
        return None;
    }
    let target = match intent {
        ReplyIntent::Create => match receipt? {
            CreationRequestReceipt::SessionCreated { session_id } => session_id,
            CreationRequestReceipt::ReceiptUnavailable => return None,
        },
        ReplyIntent::AddTab { session_id } => session_id,
        ReplyIntent::None => return None,
    };
    let session = reply.sessions.iter().find(|session| session.id == target)?;
    let tab = reply.selection.tab_id.filter(|tab| {
        reply.selection.session_id == Some(target)
            && session.tabs.iter().any(|item| item.id == *tab)
    });
    Some(ReplyFocus {
        session: target,
        tab,
    })
}
#[cfg(test)]
mod tests;
