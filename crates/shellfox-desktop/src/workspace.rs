use crate::root::Root;
mod attach;
mod reply;
mod selection;
pub(crate) mod tabs;
pub(crate) mod visibility;
mod state;
pub(crate) use state::{ActivationFence, RecentSelection, WorkspaceState};
use gpui::{Context, Window};
use shellfox_host::{AddTab, HostRequest, ProfileId, SessionId, TabId};
use shellfox_service::native::{NativeAction, NativeResponse};

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
    fn local_activation_fence(&self) -> ActivationFence {
        ActivationFence {
            source: self.source_selection_epoch(),
            navigation: self.workspace.navigation.epoch(),
        }
    }

    pub(crate) fn default_profile(&self) -> String {
        let saved = self.snapshot
            .as_ref()
            .map_or("", |snapshot| snapshot.preferences.default_profile_id.as_str());
        let usable = self.workspace.profiles.iter().filter(|p| p.available && !p.elevation).map(|p| p.id.as_str());
        // Before the catalog arrives, keep the saved one unless it needs elevation.
        shellfox_service::effective_default_profile(saved, usable)
            .unwrap_or(match saved {
                "" | shellfox_terminal::elevated::ADMINISTRATOR_PROFILE_ID => "login-shell",
                saved => saved,
            })
            .to_owned()
    }
    /// Refreshes the snapshot whenever the host commits a newer revision.
    pub(crate) fn watch_workspace(
        &self,
        client: crate::host_client::HostClient,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Task<()> {
        cx.spawn_in(window, async move |root, cx| {
            let mut revisions = client.revisions();
            while let Ok(revision) = revisions.next().await {
                // Settings changes bump the revision too; the page size lives in settings.
                let page_size = match client
                    .execute(NativeAction::Settings(shellfox_host::SettingsRequest::Get {}))
                    .await
                {
                    Ok(NativeResponse::Settings(settings)) => Some(settings.history_page_size as usize),
                    _ => None,
                };
                let current = root.update_in(cx, |root, window, cx| {
                    if page_size.is_some_and(|size| root.apply_history_page_size(size)) {
                        cx.notify();
                    }
                    if root.snapshot.as_ref().is_none_or(|snapshot| snapshot.revision < revision) {
                        root.refresh_snapshot(window, cx);
                    }
                    !root.close.closing
                });
                if !matches!(current, Ok(true)) {
                    return;
                }
            }
        })
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
        if self.close.closing {
            return;
        }
        let busy = match after {
            AfterAction::Refresh => {
                self.workspace.background.refresh_again |= self.workspace.background.refresh;
                self.workspace.background.refresh
            }
            AfterAction::Remember => self.workspace.background.remember,
            // User actions stay exclusive; their controls are locked meanwhile.
            _ => self.workspace.action_pending,
        };
        if busy {
            return;
        }
        let Some(client) = self.host.clone() else {
            self.error = Some("Shared host is not ready.".into());
            cx.notify();
            return;
        };
        match after {
            AfterAction::Refresh => self.workspace.background.refresh = true,
            AfterAction::Remember => self.workspace.background.remember = true,
            _ => {
                self.workspace.action_pending = true;
                self.dialogs.user_action_pending = true;
            }
        }
        let selection_fence = self.local_activation_fence();
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
        let cleared_session = match &action {
            NativeAction::ClearSessionError { session } => Some(*session),
            _ => None,
        };
        let activated_session = match &action {
            NativeAction::Metadata(HostRequest::ActivateSession { session_id }) => {
                Some((*session_id, self.local_activation_fence()))
            }
            _ => None,
        };
        if activated_session.is_some() {
            self.workspace.activating_session = activated_session;
        }
        let before_close = match &action {
            NativeAction::CloseTab { tab }
            | NativeAction::CloseTerminal(shellfox_core::TerminalIdentity { tab_id: tab, .. }) => self
                .snapshot
                .as_ref()
                .and_then(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .find(|session| session.tabs.iter().any(|item| item.id == *tab))
                })
                .map(|session| {
                    let mut session = session.clone();
                    session.tabs.retain(|tab| self.local_tab_viewable(tab));
                    (session, *tab)
                }),
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
                match after {
                    AfterAction::Refresh => root.workspace.background.refresh = false,
                    AfterAction::Remember => root.workspace.background.remember = false,
                    _ => {
                        root.workspace.action_pending = false;
                        root.dialogs.user_action_pending = false;
                    }
                }
                if activated_session.is_some() {
                    root.workspace.activating_session = None;
                }
                match result {
                    Ok(NativeResponse::Snapshot(snapshot)) => {
                        let selection_current = root.local_activation_fence() == selection_fence;
                        let receipt_value = receipt.as_ref().and_then(|result| result.as_ref().ok()).copied();
                        let focus = reply::focus_target(reply_intent, receipt_value, &snapshot, selection_current);
                        let receipt_unavailable = creation_request.is_some() && !matches!(receipt_value, Some(shellfox_host::CreationRequestReceipt::SessionCreated {..}));
                        let selected_tab_closed = selection_current && before_close.as_ref().is_some_and(|(before, tab)| {
                            root.workspace.navigation.tab_closed(before, *tab, selection_fence.navigation)
                        });
                        let renamed_tab = match root.dialogs.current {
                            Some(crate::dialogs::Dialog::RenameTab { id, .. }) if matches!(after, AfterAction::Dialog) => Some(id),
                            _ => None,
                        };
                        let renamed_session = match root.dialogs.current {
                            Some(crate::dialogs::Dialog::RenameSession { id, .. }) if matches!(after, AfterAction::Dialog) => Some(id),
                            _ => None,
                        };
                        let open_history = activated_session.filter(|request| {
                            reply::activation_reopens_history(
                                *request,
                                root.selected_session(),
                                root.local_activation_fence(),
                                &snapshot,
                            )
                        });
                        let preferences_changed = root
                            .snapshot
                            .as_ref()
                            .is_none_or(|old| old.preferences != snapshot.preferences);
                        if let Some(id) = cleared_session && let Some(session) = snapshot.sessions.iter().find(|session| session.id == id) {
                            for tab in &session.tabs {
                                root.tabs.failures.remove(&tab.id);
                                if root.tabs.views.get(&tab.id).is_some_and(|view| view.read(cx).is_failed()) {
                                    root.tabs.views.remove(&tab.id);
                                }
                            }
                        }
                        if root.accept_snapshot(snapshot, cx) {
                            if !matches!(after, AfterAction::Refresh | AfterAction::Remember) {
                                root.error = None;
                            }
                            if preferences_changed {
                                root.apply_terminal_preferences(window, cx);
                            }
                            if matches!(after, AfterAction::Dialog | AfterAction::Create) {
                                root.dialogs.current = None;
                            }
                            if matches!(after, AfterAction::Create | AfterAction::NewTab)
                                && let Some(target) = focus
                                && let Some(snapshot) = &root.snapshot {
                                if let Some(tab) = target.tab {
                                    root.workspace.navigation.select_tab(snapshot, target.session, tab);
                                } else {
                                    root.workspace.navigation.select_session(snapshot, target.session);
                                }
                                root.reveal_sidebar_session(target.session, cx);
                                root.ensure_terminal(window, cx);
                            } else if selected_tab_closed {
                                root.ensure_terminal(window, cx);
                            } else if selection_current && matches!(after, AfterAction::Dialog) {
                                if let Some(tab) = renamed_tab {
                                    root.focus_tab_label(tab, window, cx);
                                } else if let Some(id) = renamed_session {
                                    root.sidebar.focus_row(id, window);
                                } else {
                                    root.focus_selected(window, cx);
                                }
                            }
                            if receipt_unavailable {
                                root.error = Some("Session reply received, but its original target receipt is unavailable; this window's selection was not changed.".into());
                            }
                            if let Some((id, _)) = open_history {
                                root.add_tab(id, window, cx);
                            }
                        }
                    }
                    Ok(_) => {
                        root.error = Some("Host returned an unexpected metadata response.".into())
                    }
                    Err(error) if matches!(after, AfterAction::Remember) => {
                        if activated_session.is_some_and(|(id, fence)| {
                            root.selected_session() == Some(id) && root.local_activation_fence() == fence
                        }) {
                            root.notify_operation_error(&error, cx);
                        }
                    },
                    Err(error) if matches!(after, AfterAction::Refresh) && root.snapshot.is_none() => {
                        root.notify_error(Some("initial-workspace".into()), None, format!("Host operation failed: {error:#}"), cx);
                    }
                    Err(_) if matches!(after, AfterAction::Refresh) => {},
                    Err(error) => root.notify_operation_error(&error, cx),
                }
                if std::mem::take(&mut root.workspace.background.refresh_again) {
                    root.refresh_snapshot(window, cx);
                }
                root.flush_recent(window, cx);
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
        self.submit_sidebar_action(
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
        self.submit_sidebar_action(
            NativeAction::SetSettled {
                session: id,
                settled,
            },
            AfterAction::Mutation,
            window,
            cx,
        );
    }
    pub(crate) fn add_tab(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        let profile = self.snapshot.as_ref()
            .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
            .and_then(|session| self.profile_for_directory(&session.cwd))
            .and_then(|profile| ProfileId::new(profile).ok());
        let Some(profile_id) = profile else {
            self.error = Some("No discovered interactive terminal profile is available.".into());
            cx.notify();
            return;
        };
        self.submit_native(
            NativeAction::Metadata(HostRequest::AddTab {
                session_id: id,
                input: AddTab { title: None, cwd: None, profile_id },
            }),
            AfterAction::NewTab,
            window,
            cx,
        );
    }
}
