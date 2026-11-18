use super::*;
use crate::{app::TerminalView, host_client::HostClient, root::StartupError};
impl Root {
    pub(crate) fn ensure_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_tab() else {
            return;
        };
        if self.is_ssh_tab(id) {
            self.ensure_ssh_view(id, window, cx);
            return;
        }
        if self.views.contains_key(&id) {
            self.focus_selected(window, cx);
            return;
        }
        if self.attaching.contains(&id) {
            return;
        }
        let Some(client) = self.host.clone() else {
            return;
        };
        let selection_epoch = self.navigation.epoch();
        let focus_before = window.focused(cx);
        self.attaching.insert(id);
        self.failures.remove(&id);
        cx.spawn_in(window, async move |root, cx| {
            let attached = client.attach(id, false).await;
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
                        match TerminalView::open_attached(
                            client.clone(),
                            id,
                            attachment.clone(),
                            window,
                            cx,
                        ) {
                            Ok(view) => {
                                let marker=root.snapshot.as_ref().and_then(|snapshot|snapshot.sessions.iter().find(|session|session.tabs.iter().any(|tab|tab.id==id))).map(|session|session.settled_at.clone());
                                let Some(marker)=marker else{detach(client.clone(),id,attachment,cx);root.error=Some("Session input context is unavailable; the view was not admitted.".into());cx.notify();return;};
                                view.update(cx,|view,cx|view.set_captured_settlement(marker,cx));
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
                        let error = StartupError {
                            message: format!("Terminal attachment failed: {error:#}"),
                        };
                        root.error = Some(error.message.clone());
                        root.failures.insert(id, error);
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
    attachment: shellfox_service::Attachment,
    cx: &mut Context<Root>,
) {
    cx.spawn(async move |root, cx| {
        if let Err(error) = client.detach(id, attachment).await {
            let _ = root.update(cx, |root, cx| {
                root.error = Some(format!("View detach failed: {error:#}"));
                cx.notify();
            });
        }
    })
    .detach();
}
