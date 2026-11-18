//! Window-local SSH intent/status routing over the originating native client.
#[path = "root_ssh_poll.rs"]
mod poll;
use crate::{
    app::TerminalView,
    host_client::HostClient,
    root::Root,
    settings::{SettingsSshOpenFailed, SettingsSshOpened, SshOpenAdmission},
};
use gpui::{Context, Subscription, Window};
use shellfox_core::{SessionId, TabId};
use shellfox_service::{
    native::{NativeAction, NativeResponse},
    ssh::{self, ConnectionStatus, State},
};

#[derive(Default)]
pub(crate) struct SshUi {
    pub subscriptions: Vec<Subscription>,
    pending: Option<Intent>,
    latest: Option<SessionId>,
    target: Option<Target>,
}
fn take_intent(pending: &mut Option<Intent>, id: SessionId) -> Option<Intent> {
    if pending.as_ref().is_some_and(|intent| intent.id == id) {
        pending.take()
    } else {
        None
    }
}

struct Intent {
    id: SessionId,
    epoch: u64,
    profile: String,
}
struct Target {
    intent: Option<SessionId>,
    tab: TabId,
    client: HostClient,
    status: Option<ConnectionStatus>,
    epoch: u64,
    busy: bool,
    ready_attempted: bool,
}
impl Root {
    // Routing tag only: readiness/visibility always comes from scoped SSH status
    // and its validated runtime identity, never this metadata profile spelling.
    pub(crate) fn is_ssh_tab(&self, tab: TabId) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.sessions.iter().any(|session| {
                session
                    .tabs
                    .iter()
                    .any(|item| item.id == tab && item.profile_id.as_str().starts_with("ssh-"))
            })
        })
    }
    pub(crate) fn ssh_status(&self, tab: TabId) -> Option<String> {
        if !self.is_ssh_tab(tab) {
            return None;
        }
        let state = self
            .ssh_ui
            .target
            .as_ref()
            .filter(|target| target.tab == tab)
            .and_then(|target| target.status.as_ref())
            .map(|status| &status.state);
        Some(
            match state {
                None => "SSH status is not visible in this window (read-only)",
                Some(State::Ready) => "SSH is ready; waiting for checked presentation access",
                Some(State::AwaitingHost(_)) => "SSH is waiting for host verification",
                Some(State::AwaitingPrompt(_)) => "SSH is waiting for authentication",
                Some(State::Closed) => "SSH connection is closed",
                Some(State::Failed { .. }) => "SSH connection failed",
                _ => "Preparing SSH connection (read-only)",
            }
            .into(),
        )
    }
    pub(crate) fn open_saved_ssh(&mut self, profile: String, cx: &mut Context<Self>) {
        if self.ssh_ui.pending.is_some() || self.closing_window {
            self.error =
                Some("An SSH opening is already pending or this window is closing.".into());
            cx.notify();
            return;
        }
        let Some(panel) = &self.settings_panel else {
            return;
        };
        let Ok(id) = profile.parse() else {
            self.error = Some("Invalid SSH profile identity".into());
            cx.notify();
            return;
        };
        let request_id = SessionId::new();
        let admission = panel.update(cx, |panel, cx| {
            panel.open_ssh_profile(
                ssh::OpenProfile {
                    request_id,
                    profile: id,
                    cols: 80,
                    rows: 24,
                },
                cx,
            )
        });
        match admission {
            SshOpenAdmission::Queued {
                request_id: accepted,
            } if accepted == request_id => {
                self.ssh_ui.latest = Some(request_id);
                self.ssh_ui.pending = Some(Intent {
                    id: request_id,
                    epoch: self.navigation.epoch(),
                    profile,
                });
            }
            SshOpenAdmission::Rejected { reason, .. } => {
                self.error = Some(format!("SSH opening was not admitted: {reason:?}"))
            }
            _ => self.error = Some("SSH opening admission identity mismatch".into()),
        }
        cx.notify();
    }
    pub(crate) fn ssh_open_failed(
        &mut self,
        event: &SettingsSshOpenFailed,
        cx: &mut Context<Self>,
    ) {
        if take_intent(&mut self.ssh_ui.pending, event.request_id).is_some() {
            self.error = Some(event.message.clone());
            cx.notify();
        }
    }
    pub(crate) fn ssh_opened(
        &mut self,
        event: &SettingsSshOpened,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(intent) = take_intent(&mut self.ssh_ui.pending, event.request_id) else {
            return;
        };
        if intent.profile != event.opened.connection.profile.to_string() {
            self.error = Some("SSH profile completion identity mismatch".into());
            cx.notify();
            return;
        }
        let status = event.opened.connection.clone();
        let client = event.client.clone();
        cx.spawn_in(window, async move |root, cx| {
            let snapshot = client
                .execute(NativeAction::Metadata(
                    shellfox_host::HostRequest::GetSnapshot {},
                ))
                .await;
            let _ = root.update_in(cx, |root, _, cx| {
                if root.closing_window
                    || root.navigation.epoch() != intent.epoch
                    || root.ssh_ui.latest != Some(intent.id)
                {
                    return;
                }
                let Ok(NativeResponse::Snapshot(snapshot)) = snapshot else {
                    root.error = Some("Fresh SSH target snapshot is unavailable".into());
                    cx.notify();
                    return;
                };
                root.accept_snapshot(snapshot, cx);
                let Some(snapshot) = &root.snapshot else {
                    return;
                };
                let Some(session) = snapshot.sessions.iter().find(|session| {
                    session.tabs.iter().any(|tab| {
                        tab.id == status.tab
                            && tab.profile_id.as_str() == format!("ssh-{}", status.profile)
                    })
                }) else {
                    root.error = Some("Original SSH tab is unavailable".into());
                    cx.notify();
                    return;
                };
                if !root.navigation.select_tab(snapshot, session.id, status.tab) {
                    return;
                }
                root.settings = false;
                root.pending_recent = None;
                root.ssh_ui.target = Some(Target {
                    intent: Some(intent.id),
                    tab: status.tab,
                    client,
                    status: Some(status),
                    epoch: root.navigation.epoch(),
                    busy: false,
                    ready_attempted: false,
                });
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn ensure_ssh_view(
        &mut self,
        tab: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = &mut self.ssh_ui.target
            && target.tab == tab
        {
            if target.epoch != self.navigation.epoch() {
                target.busy = false;
            }
            target.intent = self.ssh_ui.latest;
            target.epoch = self.navigation.epoch();
            target.ready_attempted = false;
            self.poll_ssh(window, cx);
            return;
        }
        if let Some(client) = &self.host {
            self.ssh_ui.target = Some(Target {
                intent: self.ssh_ui.latest,
                tab,
                client: client.clone(),
                status: None,
                epoch: self.navigation.epoch(),
                busy: false,
                ready_attempted: false,
            });
            self.poll_ssh(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completions_clear_only_original_request_not_newer_intent() {
        let old = SessionId::new();
        let current = SessionId::new();
        let mut pending = Some(Intent {
            id: current,
            epoch: 7,
            profile: "fixture".into(),
        });
        assert!(take_intent(&mut pending, old).is_none());
        assert_eq!(pending.as_ref().unwrap().id, current);
        let completed = take_intent(&mut pending, current).unwrap();
        assert_eq!(completed.id, current);
        assert_eq!(completed.epoch, 7);
        assert!(pending.is_none());
        assert!(take_intent(&mut pending, current).is_none());
    }
}
