use super::*;
use crate::{app::{TerminalRecovery, TerminalView}, host_client::HostClient, root::StartupError};
use shellfox_service::native::NativeViewAttachment;
impl Root {
    pub(crate) fn ensure_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_tab() else {
            return;
        };
        self.attach_local_view(id, None, window, cx);
    }

    pub(crate) fn attach_local_view(&mut self, id: TabId, recovery: Option<TerminalRecovery>, window: &mut Window, cx: &mut Context<Self>) {
        if recovery.is_none() && self.views.contains_key(&id) {
            self.focus_selected(window, cx);
            return;
        }
        if self.attaching.contains(&id) {
            return;
        }
        let Some(client) = self.host.clone() else {
            return;
        };
        let read_only = self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.sessions.iter().flat_map(|session| &session.tabs).any(|tab|
                tab.id == id && tab.profile_id.as_str() == shellfox_service::AUTOMATION_PROFILE_ID
            )
        });
        let takeover = recovery == Some(TerminalRecovery::TakeControl);
        if read_only && takeover {
            return;
        }
        let selection_epoch = self.navigation.epoch();
        let focus_before = window.focused(cx);
        self.attaching.insert(id);
        if recovery != Some(TerminalRecovery::TakeControl) {
            self.failures.remove(&id);
        }
        cx.spawn_in(window, async move |root, cx| {
            let attached = if read_only {
                client.execute(NativeAction::Observe { tab: id }).await.and_then(|response| {
                    match response {
                        NativeResponse::Observed(attachment) => Ok(NativeViewAttachment::Observer(attachment)),
                        _ => anyhow::bail!("Service returned an invalid read-only terminal attachment."),
                    }
                })
            } else {
                client.attach(id, takeover).await.map(NativeViewAttachment::Controller)
            };
            let _ = root.update_in(cx, |root, window, cx| {
                root.attaching.remove(&id);
                match attached {
                    Ok(attachment) => {
                        if !root.snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot
                                .sessions
                                .iter()
                                .any(|session| session.tabs.iter().any(|tab| tab.id == id))
                        }) {
                            detach(client.clone(), id, attachment, cx);
                            return;
                        }
                        if root.navigation.epoch() != selection_epoch
                            || root.selected_tab() != Some(id)
                            || root.settings
                            || root.dialog.is_some()
                            || window.focused(cx) != focus_before
                        {
                            detach(client.clone(), id, attachment, cx);
                            cx.notify();
                            return;
                        }
                        let opened = match &attachment {
                            NativeViewAttachment::Controller(handle) => TerminalView::open_attached(client.clone(), id, handle.clone(), window, cx),
                            NativeViewAttachment::Observer(handle) => TerminalView::open_observed(client.clone(), id, handle.clone(), window, cx),
                        };
                        match opened {
                            Ok(view) => {
                                let marker=root.snapshot.as_ref().and_then(|snapshot|snapshot.sessions.iter().find(|session|session.tabs.iter().any(|tab|tab.id==id))).map(|session|session.settled_at.clone());
                                let Some(marker)=marker else{detach(client.clone(),id,attachment,cx);root.error=Some("Session input context is unavailable; the view was not admitted.".into());cx.notify();return;};
                                view.update(cx,|view,cx|view.set_captured_settlement(marker,cx));
                                cx.subscribe_in(&view, window, move |root, emitter, event: &TerminalRecovery, window, cx| {
                                    if root.selected_tab() == Some(id) && root.views.get(&id) == Some(&emitter) {
                                        root.attach_local_view(id, Some(*event), window, cx);
                                    }
                                }).detach();
                                if matches!(cx.try_global::<crate::editor_composition::EditorOutcome>(), Some(crate::editor_composition::EditorOutcome::Ready(_))) {
                                    view.update(cx, |view, _| view.enable_file_links());
                                    cx.subscribe_in(&view, window, move |root, emitter, event: &crate::app::TerminalFileLink, window, cx| {
                                        if root.views.get(&id) == Some(&emitter) {
                                            root.open_terminal_file_link(id, event, window, cx);
                                        }
                                    }).detach();
                                }
                                root.failures.remove(&id);
                                root.views.insert(id, view);
                                root.apply_terminal_preferences(window, cx);
                                if root.selected_tab() == Some(id) {
                                    root.focus_selected(window, cx);
                                }
                            }
                            Err(error) => {
                                detach(client.clone(), id, attachment, cx);
                                root.error = Some(error.message.clone());
                                root.failures.insert(id, error);
                            }
                        }
                    }
                    Err(error) => {
                        root.failures.insert(id, StartupError::from(error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
fn detach(
    client: HostClient,
    id: TabId,
    attachment: NativeViewAttachment,
    cx: &mut Context<Root>,
) {
    cx.spawn(async move |root, cx| {
        let result = match attachment {
            NativeViewAttachment::Controller(handle) => client.detach(id, handle).await,
            NativeViewAttachment::Observer(handle) => client.execute(NativeAction::Unobserve {
                tab: id, attachment: handle,
            }).await.map(|_| ()),
        };
        if let Err(error) = result {
            let _ = root.update(cx, |root, cx| {
                root.error = Some(format!("View detach failed: {error:#}"));
                cx.notify();
            });
        }
    })
    .detach();
}
