use crate::{
    host_client::HostClient,
    root::{Dialog, Root},
    workspace::AfterAction,
};
use gpui::{Context, Window};
use shellfox_host::{HostRequest, SessionId};
use shellfox_service::native::NativeAction;

#[path = "workspace_refresh_slot.rs"]
mod slot;
use slot::RefreshSlot;

#[derive(Default)]
pub(crate) struct RefreshAdmission(RefreshSlot<DeferredTab>);

pub(super) struct DeferredTab {
    session: SessionId,
    action: NativeAction,
    after: AfterAction,
    selection_epoch: u64,
    source_epoch: u64,
}

impl RefreshAdmission {
    pub(super) fn begin(&mut self) {
        self.0 = RefreshSlot::Reading;
    }

    pub(super) fn finish(&mut self) -> Option<DeferredTab> {
        self.0.finish()
    }
}

impl Root {
    pub(super) fn reserve_tab_action(&mut self, action: NativeAction, after: AfterAction) -> bool {
        let Some(selected) = self.selected_session() else {
            return false;
        };
        let session = match (&action, after) {
            (
                NativeAction::Metadata(HostRequest::AddTab { session_id, .. }),
                AfterAction::NewTab,
            )
            | (
                NativeAction::Metadata(HostRequest::ReorderTabs { session_id, .. }),
                AfterAction::Mutation,
            )
            | (
                NativeAction::Metadata(HostRequest::RenameTab { session_id, .. }),
                AfterAction::Dialog,
            ) => *session_id,
            (NativeAction::CloseTerminal(target), AfterAction::Dialog | AfterAction::Mutation) => {
                if !self.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.sessions.iter().any(|session| {
                        session.id == selected
                            && session.tabs.iter().any(|candidate| candidate.id == target.tab_id)
                    })
                }) {
                    return false;
                }
                selected
            }
            _ => return false,
        };
        if session != selected {
            return false;
        }
        let deferred = DeferredTab {
            session,
            action,
            after,
            selection_epoch: self.navigation.epoch(),
            source_epoch: self.source_selection_epoch(),
        };
        if self.refresh_admission.0.reserve(deferred) {
            self.user_action_pending = true;
            true
        } else {
            false
        }
    }

    pub(super) fn submit_reserved_tab(
        &mut self,
        tab: DeferredTab,
        refresh_client: &HostClient,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing_window || self.close_check_pending {
            return;
        }
        let dialog_current = match (&tab.action, &self.dialog) {
            (
                NativeAction::Metadata(HostRequest::RenameTab { tab_id, .. }),
                Some(Dialog::RenameTab { id, .. }),
            ) => tab_id == id,
            (NativeAction::CloseTerminal(target), Some(Dialog::CloseTab(confirmed))) => target == confirmed,
            (_, None) => !matches!(tab.after, AfterAction::Dialog),
            _ => false,
        };
        if self.settings
            || !dialog_current
            || self.selected_session() != Some(tab.session)
            || self.navigation.epoch() != tab.selection_epoch
            || self.source_selection_epoch() != tab.source_epoch
            || !self
                .host
                .as_ref()
                .is_some_and(|current| current.same_window_client(refresh_client))
        {
            self.error = Some("Terminal action canceled because the workspace changed.".into());
            return;
        }
        self.submit_native(tab.action, tab.after, window, cx);
    }
}
