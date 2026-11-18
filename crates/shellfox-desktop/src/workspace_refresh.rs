use crate::{host_client::HostClient, root::Root, workspace::AfterAction};
use gpui::{Context, Window};
use shellfox_host::{AddTab, HostRequest, SessionId};
use shellfox_service::native::NativeAction;

#[path = "workspace_refresh_slot.rs"]
mod slot;
use slot::RefreshSlot;

#[derive(Default)]
pub(crate) struct RefreshAdmission(RefreshSlot<DeferredTab>);

pub(super) struct DeferredTab {
    session: SessionId,
    input: AddTab,
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
    pub(super) fn reserve_new_tab(&mut self, action: NativeAction, after: AfterAction) -> bool {
        let (
            AfterAction::NewTab,
            NativeAction::Metadata(HostRequest::AddTab { session_id, input }),
        ) = (after, action)
        else {
            return false;
        };
        if self.selected_session() != Some(session_id) {
            return false;
        }
        let tab = DeferredTab {
            session: session_id,
            input,
            selection_epoch: self.navigation.epoch(),
            source_epoch: self.source_selection_epoch(),
        };
        self.refresh_admission.0.reserve(tab)
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
        if self.settings
            || self.dialog.is_some()
            || self.selected_session() != Some(tab.session)
            || self.navigation.epoch() != tab.selection_epoch
            || self.source_selection_epoch() != tab.source_epoch
            || !self
                .host
                .as_ref()
                .is_some_and(|current| current.same_window_client(refresh_client))
        {
            self.error = Some("New terminal canceled because the workspace changed.".into());
            return;
        }
        self.submit_native(
            NativeAction::Metadata(HostRequest::AddTab {
                session_id: tab.session,
                input: tab.input,
            }),
            AfterAction::NewTab,
            window,
            cx,
        );
    }
}
