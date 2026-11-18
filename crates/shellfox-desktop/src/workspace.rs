use crate::root::Root;
#[path = "workspace_attach.rs"]
mod attach;
#[path = "workspace_refresh.rs"]
pub(crate) mod refresh;
#[path = "workspace_reply.rs"]
mod reply;
use gpui::{Context, Window};
use shellfox_host::{AddTab, HostRequest, ProfileId, SessionId, TabId};
use shellfox_service::native::{NativeAction, NativeResponse};

#[derive(Clone, Copy)]
pub(crate) enum RecentSelection {
    Session(SessionId),
    Tab(TabId),
}

#[derive(Clone, Copy)]
pub(crate) enum AfterAction {
    Refresh,
    Mutation,
    Remember,
    NewTab,
    Dialog,
    Create,
}
impl Root {
    pub(crate) fn default_profile(&self) -> String {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.preferences.default_profile_id.as_str())
            .unwrap_or("login-shell")
            .to_owned()
    }
    pub(crate) fn refresh_snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit_native(
            NativeAction::Metadata(HostRequest::GetSnapshot {}),
            AfterAction::Refresh,
            window,
            cx,
        );
    }
    pub(crate) fn submit_native(
        &mut self,
        action: NativeAction,
        after: AfterAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing_window {
            return;
        }
        if self.action_pending {
            if !matches!(after, AfterAction::Refresh) && !self.reserve_new_tab(action, after) {
                self.error =
                    Some("A host operation is still pending. Retry after it completes.".into());
                cx.notify();
            }
            return;
        }
        let Some(client) = self.host.clone() else {
            self.error = Some("Shared host is not ready.".into());
            cx.notify();
            return;
        };
        self.action_pending = true;
        self.user_action_pending = !matches!(after, AfterAction::Refresh | AfterAction::Remember);
        if matches!(after, AfterAction::Refresh) {
            self.refresh_admission.begin();
        }
        let selection_epoch = self.navigation.epoch();
        let (reply_intent, creation_request) = match &action {
            NativeAction::Metadata(HostRequest::CreateSession(request)) => {
                (reply::ReplyIntent::Create, Some(request.request_id))
            }
            NativeAction::Metadata(HostRequest::AddTab { session_id, .. }) => (
                reply::ReplyIntent::AddTab {
                    session_id: *session_id,
                },
                None,
            ),
            _ => (reply::ReplyIntent::None, None),
        };
        let before_close = match &action {
            NativeAction::CloseTab { tab } => self
                .snapshot
                .as_ref()
                .and_then(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .find(|session| session.tabs.iter().any(|item| item.id == *tab))
                })
                .map(|session| (session.clone(), *tab)),
            _ => None,
        };
        cx.spawn_in(window, async move |root, cx| {
            let result = client.execute(action).await;
            // Query only after a successful create reply, on the SAME originating
            // client. Pending/epoch fences remain held throughout this read.
            let receipt = if let Some(request_id) = creation_request
                && matches!(&result, Ok(NativeResponse::Snapshot(_))) {
                Some(client.creation_request_receipt(request_id).await)
            } else {None};
            let _ = root.update_in(cx, |root, window, cx| {
                let reserved_tab = if matches!(after, AfterAction::Refresh) {
                    root.refresh_admission.finish()
                } else {
                    None
                };
                root.action_pending = false;
                root.user_action_pending = false;
                match result {
                    Ok(NativeResponse::Snapshot(snapshot)) => {
                        let selection_current = root.navigation.epoch() == selection_epoch;
                        let receipt_value = receipt.as_ref().and_then(|result| result.as_ref().ok()).copied();
                        let focus = reply::focus_target(reply_intent, receipt_value, &snapshot, selection_current);
                        let receipt_unavailable = creation_request.is_some() && !matches!(receipt_value, Some(shellfox_host::CreationRequestReceipt::SessionCreated {..}));
                        if let Some((before, tab)) = &before_close {
                            root.navigation.tab_closed(before, *tab, selection_epoch);
                        }
                        let preferences_changed = root
                            .snapshot
                            .as_ref()
                            .is_none_or(|old| old.preferences != snapshot.preferences);
                        if root.accept_snapshot(snapshot, cx) {
                            if !matches!(after, AfterAction::Refresh | AfterAction::Remember) {
                                root.error = None;
                            }
                            if preferences_changed {
                                root.apply_terminal_preferences(window, cx);
                            }
                            if matches!(after, AfterAction::Dialog | AfterAction::Create) {
                                root.dialog = None;
                            }
                            if matches!(after, AfterAction::Create | AfterAction::NewTab)
                                && let Some(target) = focus
                                && let Some(snapshot) = &root.snapshot {
                                if let Some(tab) = target.tab {
                                    root.navigation.select_tab(snapshot, target.session, tab);
                                } else {
                                    root.navigation.select_session(snapshot, target.session);
                                }
                                root.ensure_terminal(window, cx);
                            } else if selection_current && matches!(after, AfterAction::Dialog) {
                                root.focus_selected(window, cx);
                            }
                            if receipt_unavailable {
                                root.error = Some("Session reply received, but its original target receipt is unavailable; this window's selection was not changed.".into());
                            }
                            if let Some(tab) = reserved_tab {
                                root.submit_reserved_tab(tab, &client, window, cx);
                            }
                        } else if reserved_tab.is_some() {
                            root.error = Some("New terminal canceled because the workspace changed.".into());
                        }
                    }
                    Ok(_) => {
                        root.error = Some("Host returned an unexpected metadata response.".into())
                    }
                    Err(error) => root.error = Some(format!("Host operation failed: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn toggle_session_pinned(
        &mut self,
        id: SessionId,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_native(
            NativeAction::Metadata(HostRequest::SetSessionPinned {
                session_id: id,
                pinned,
            }),
            AfterAction::Mutation,
            window,
            cx,
        );
    }
    pub(crate) fn toggle_session_settled(
        &mut self,
        id: SessionId,
        settled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_native(
            NativeAction::SetSettled {
                session: id,
                settled,
            },
            AfterAction::Mutation,
            window,
            cx,
        );
    }
    pub(crate) fn select_session(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(feature="experimental-shellfox-native")]
        if !self.is_local_source(){self.select_local_source(window,cx);}
        if self.closing_window {
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        if !self.navigation.select_session(snapshot, id) {
            return;
        }
        self.settings = false;
        self.pending_recent = Some(RecentSelection::Session(id));
        self.ensure_terminal(window, cx);
        self.flush_recent(window, cx);
        cx.notify();
    }
    pub(crate) fn select_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing_window {
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let Some(session) = snapshot
            .sessions
            .iter()
            .find(|session| session.tabs.iter().any(|tab| tab.id == id))
        else {
            return;
        };
        if !self.navigation.select_tab(snapshot, session.id, id) {
            return;
        }
        self.pending_recent = Some(RecentSelection::Tab(id));
        self.ensure_terminal(window, cx);
        self.flush_recent(window, cx);
        cx.notify();
    }
    pub(crate) fn flush_recent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.action_pending || self.closing_window {
            return;
        }
        let Some(recent) = self.pending_recent.take() else {
            return;
        };
        let action = match recent {
            RecentSelection::Session(id) => {
                NativeAction::Metadata(HostRequest::ActivateSession { session_id: id })
            }
            RecentSelection::Tab(id) => NativeAction::SelectTab { tab: id },
        };
        self.submit_native(action, AfterAction::Remember, window, cx);
    }
    pub(crate) fn add_tab(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        match ProfileId::new(self.default_profile()) {
            Ok(profile_id) => self.submit_native(
                NativeAction::Metadata(HostRequest::AddTab {
                    session_id: id,
                    input: AddTab {
                        title: None,
                        cwd: None,
                        profile_id,
                    },
                }),
                AfterAction::NewTab,
                window,
                cx,
            ),
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }
}
