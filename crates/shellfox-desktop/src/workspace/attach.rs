use super::*;
use crate::{
    host_client::HostClient,
    root::StartupError,
    terminal::{TerminalRecovery, TerminalView},
};
use shellfox_core::Generation;
use shellfox_service::native::NativeViewAttachment;

/// What a finished attach does with its view. Views are per tab, so a view is
/// kept even when the user moved on; only taking focus depends on the moment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Landing {
    Detach,
    Keep { focus: bool },
}

#[derive(Clone, Copy, Debug, Default)]
struct LandingFacts {
    tab_exists: bool,
    selected: bool,
    settings: bool,
    dialog: bool,
    focus_moved: bool,
}

fn landing(facts: LandingFacts) -> Landing {
    if !facts.tab_exists {
        return Landing::Detach;
    }
    Landing::Keep {
        focus: facts.selected && !facts.settings && !facts.dialog && !facts.focus_moved,
    }
}

/// A selected tab needs an attach unless it has a view, one is in flight, or it
/// failed for the terminal generation that is still current.
fn attach_due(
    has_view: bool,
    attaching: bool,
    failed_at: Option<Option<Generation>>,
    current: Option<Generation>,
) -> bool {
    !has_view && !attaching && failed_at.is_none_or(|generation| generation != current)
}

impl Root {
    pub(crate) fn ensure_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_tab() else {
            return;
        };
        self.attach_local_view(id, None, window, cx);
    }

    /// Called on every render: the selected terminal attaches without a click.
    /// Deferred because render can't mutate; idempotent through `attaching` and
    /// per-generation failures, so it neither repeats nor retries a failure.
    pub(crate) fn ensure_selected_views(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.local_attach_due().is_none() && !self.peer_attach_due() {
            return;
        }
        cx.defer_in(window, |root, window, cx| {
            if let Some(id) = root.local_attach_due() {
                root.attach_local_view(id, None, window, cx);
            }
            if root.peer_attach_due() {
                root.open_v1_terminal(window, cx);
            }
        });
    }

    fn local_attach_due(&self) -> Option<TabId> {
        if self.close.closing || self.fixture.is_some() || self.host.is_none() {
            return None;
        }
        let id = self.selected_tab()?;
        attach_due(
            self.tabs.views.contains_key(&id),
            self.tabs.attaching.contains(&id),
            self.tabs
                .failures
                .get(&id)
                .map(|failure| failure.generation),
            self.terminal_generation(id),
        )
        .then_some(id)
    }

    fn terminal_generation(&self, id: TabId) -> Option<Generation> {
        self.tab_close_identity(id)
            .map(|identity| identity.generation)
    }

    pub(crate) fn attach_local_view(
        &mut self,
        id: TabId,
        recovery: Option<TerminalRecovery>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if recovery.is_none() && self.tabs.views.contains_key(&id) {
            self.focus_selected(window, cx);
            return;
        }
        if self.tabs.attaching.contains(&id) {
            return;
        }
        let Some(client) = self.host.clone() else {
            return;
        };
        let read_only = self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .sessions
                .iter()
                .flat_map(|session| &session.tabs)
                .any(|tab| {
                    tab.id == id
                        && tab.profile_id.as_str() == shellfox_service::AUTOMATION_PROFILE_ID
                })
        });
        let takeover = recovery == Some(TerminalRecovery::TakeControl);
        if read_only && takeover {
            return;
        }
        let focus_before = window.focused(cx);
        self.tabs.attaching.insert(id);
        if recovery != Some(TerminalRecovery::TakeControl) {
            self.tabs.failures.remove(&id);
        }
        cx.spawn_in(window, async move |root, cx| {
            let attached = if read_only {
                client
                    .execute(NativeAction::Observe { tab: id })
                    .await
                    .and_then(|response| match response {
                        NativeResponse::Observed(attachment) => {
                            Ok(NativeViewAttachment::Observer(attachment))
                        }
                        _ => anyhow::bail!(
                            "Service returned an invalid read-only terminal attachment."
                        ),
                    })
            } else {
                client
                    .attach(id, takeover)
                    .await
                    .map(NativeViewAttachment::Controller)
            };
            let _ = root.update_in(cx, |root, window, cx| {
                root.tabs.attaching.remove(&id);
                match attached {
                    Ok(attachment) => {
                        let facts = LandingFacts {
                            tab_exists: root.snapshot.as_ref().is_some_and(|snapshot| {
                                snapshot
                                    .sessions
                                    .iter()
                                    .any(|session| session.tabs.iter().any(|tab| tab.id == id))
                            }),
                            selected: root.selected_tab() == Some(id),
                            settings: root.settings.open,
                            dialog: root.dialogs.current.is_some(),
                            focus_moved: window.focused(cx) != focus_before,
                        };
                        match landing(facts) {
                            Landing::Detach => detach(client.clone(), id, attachment, cx),
                            Landing::Keep { focus } => {
                                root.admit_view(id, client.clone(), attachment, focus, window, cx)
                            }
                        }
                    }
                    Err(error) => {
                        let mut failure = StartupError::from(error);
                        failure.generation = root.terminal_generation(id);
                        root.tabs.failures.insert(id, failure);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn admit_view(
        &mut self,
        id: TabId,
        client: HostClient,
        attachment: NativeViewAttachment,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opened = match &attachment {
            NativeViewAttachment::Controller(handle) => {
                TerminalView::open_attached(client.clone(), id, handle.clone(), window, cx)
            }
            NativeViewAttachment::Observer(handle) => {
                TerminalView::open_observed(client.clone(), id, handle.clone(), window, cx)
            }
        };
        let view = match opened {
            Ok(view) => view,
            Err(mut error) => {
                detach(client, id, attachment, cx);
                self.error = Some(error.message.clone());
                error.generation = self.terminal_generation(id);
                self.tabs.failures.insert(id, error);
                return;
            }
        };
        let marker = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .sessions
                    .iter()
                    .find(|session| session.tabs.iter().any(|tab| tab.id == id))
            })
            .map(|session| session.settled_at.clone());
        let Some(marker) = marker else {
            detach(client, id, attachment, cx);
            self.error =
                Some("Session input context is unavailable; the view was not admitted.".into());
            return;
        };
        view.update(cx, |view, cx| view.set_captured_settlement(marker, cx));
        // The tab strip and sidebar are cached panes; they show each terminal's phase and
        // failure, which change with a notify on the view, rendered or not.
        let mut status = (view.read(cx).runtime_phase(), view.read(cx).is_failed());
        cx.observe(&view, move |_, view, cx| {
            let now = (view.read(cx).runtime_phase(), view.read(cx).is_failed());
            if std::mem::replace(&mut status, now) != now {
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(
            &view,
            window,
            move |root, emitter, event: &TerminalRecovery, window, cx| {
                if root.selected_tab() == Some(id) && root.tabs.views.get(&id) == Some(emitter) {
                    root.attach_local_view(id, Some(*event), window, cx);
                }
            },
        )
        .detach();
        if matches!(
            cx.try_global::<crate::editor_composition::EditorOutcome>(),
            Some(crate::editor_composition::EditorOutcome::Ready(_))
        ) {
            view.update(cx, |view, _| view.enable_file_links());
            cx.subscribe_in(
                &view,
                window,
                move |root, emitter, event: &crate::terminal::TerminalFileLink, window, cx| {
                    if root.tabs.views.get(&id) == Some(emitter) {
                        root.open_terminal_file_link(id, event, window, cx);
                    }
                },
            )
            .detach();
        }
        self.tabs.failures.remove(&id);
        self.tabs.views.insert(id, view);
        self.apply_terminal_preferences(window, cx);
        if focus {
            self.focus_selected(window, cx);
        }
    }
}

fn detach(client: HostClient, id: TabId, attachment: NativeViewAttachment, cx: &mut Context<Root>) {
    cx.spawn(async move |root, cx| {
        let result = match attachment {
            NativeViewAttachment::Controller(handle) => client.detach(id, handle).await,
            NativeViewAttachment::Observer(handle) => client
                .execute(NativeAction::Unobserve {
                    tab: id,
                    attachment: handle,
                })
                .await
                .map(|_| ()),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn live(selected: bool) -> LandingFacts {
        LandingFacts {
            tab_exists: true,
            selected,
            ..LandingFacts::default()
        }
    }

    #[test]
    fn an_attach_that_lands_after_focus_moved_to_the_sidebar_keeps_its_view_unfocused() {
        assert_eq!(
            landing(LandingFacts {
                focus_moved: true,
                ..live(true)
            }),
            Landing::Keep { focus: false }
        );
    }

    #[test]
    fn an_attach_that_lands_under_settings_or_a_dialog_keeps_its_view_unfocused() {
        assert_eq!(
            landing(LandingFacts {
                settings: true,
                ..live(true)
            }),
            Landing::Keep { focus: false }
        );
        assert_eq!(
            landing(LandingFacts {
                dialog: true,
                ..live(true)
            }),
            Landing::Keep { focus: false }
        );
    }

    #[test]
    fn an_attach_for_a_tab_the_user_left_keeps_its_view_for_later() {
        assert_eq!(landing(live(false)), Landing::Keep { focus: false });
    }

    #[test]
    fn an_attach_that_lands_after_window_activation_takes_focus() {
        // Activation keeps the window's focused element, so nothing moved.
        assert_eq!(landing(live(true)), Landing::Keep { focus: true });
    }

    #[test]
    fn only_a_removed_tab_detaches_its_attach() {
        assert_eq!(
            landing(LandingFacts {
                tab_exists: false,
                ..live(true)
            }),
            Landing::Detach
        );
    }

    #[test]
    fn a_restored_selected_tab_attaches_without_a_click() {
        assert!(attach_due(false, false, None, None));
        assert!(attach_due(false, false, None, Some(Generation::new())));
    }

    #[test]
    fn a_relaunched_window_attaches_the_running_tab_its_first_snapshot_selects() {
        use crate::window_manager::{InitialSelection, WindowSelection};
        use shellfox_core::{Preferences, ProfileId, Selection, Session, Snapshot, Tab, TabState};
        let session = SessionId::new();
        let tab = TabId::new();
        let snapshot = Snapshot {
            revision: 1,
            sessions: vec![Session {
                id: session,
                title: "user".into(),
                cwd: "~".into(),
                created_at: "2025-01-01T00:00:00Z".into(),
                updated_at: "2025-01-01T00:00:00Z".into(),
                settled_at: None,
                pinned_at: None,
                env: vec![],
                tabs: vec![Tab {
                    id: tab,
                    session_id: session,
                    title: "Shell 1".into(),
                    use_process_title: false,
                    cwd: "~".into(),
                    ordinal: 0,
                    created_at: None,
                    profile_id: ProfileId::default(),
                    wsl_launch: None,
                    state: TabState::Pending,
                }],
            }],
            preferences: Preferences::default(),
            selection: Selection {
                session_id: Some(session),
                tab_id: Some(tab),
            },
        };
        let mut navigation = WindowSelection::new(InitialSelection::Recent);
        assert!(navigation.accept_snapshot(&snapshot));
        assert_eq!(navigation.selected().tab_id, Some(tab));
        // No click, no view, nothing in flight, and the shell is already running.
        assert!(attach_due(false, false, None, Some(Generation::new())));
    }

    #[test]
    fn a_tab_attaches_once_and_never_while_it_has_a_view() {
        assert!(!attach_due(false, true, None, None));
        assert!(!attach_due(true, false, None, None));
    }

    #[test]
    fn a_failure_waits_for_reconnect_or_a_new_terminal_generation() {
        let generation = Generation::new();
        assert!(!attach_due(
            false,
            false,
            Some(Some(generation)),
            Some(generation)
        ));
        assert!(!attach_due(false, false, Some(None), None));
        assert!(attach_due(
            false,
            false,
            Some(Some(generation)),
            Some(Generation::new())
        ));
        assert!(attach_due(false, false, Some(None), Some(generation)));
    }
}
