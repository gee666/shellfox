//! Apply an owner-resolved launch to one Root.
use super::*;
use crate::window_manager::WindowSelection;
use shellfox_service::desktop_launch::{LaunchError, LaunchTarget};

fn select_target(
    navigation: &mut WindowSelection,
    snapshot: &Snapshot,
    target: &LaunchTarget,
) -> Result<(), LaunchError> {
    let accepted = match target {
        LaunchTarget::Show {} => return Ok(()),
        LaunchTarget::Session {
            session,
            tab: Some(tab),
        } => navigation.select_tab(snapshot, *session, *tab),
        LaunchTarget::Session { session, tab: None } => {
            navigation.select_session(snapshot, *session)
        }
    };
    if accepted {
        Ok(())
    } else {
        Err(LaunchError::NotFound)
    }
}

impl Root {
    /// The app coordinator owns request deduplication and the window disposition.
    /// Its transport supplies a fresh snapshot from the existing owner and the
    /// local epoch captured before awaiting it. A later user navigation wins.
    pub(crate) fn apply_launch_target(
        &mut self,
        snapshot: Snapshot,
        target: &LaunchTarget,
        expected_epoch: u64,
        activate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), LaunchError> {
        if self.closing_window
            || self.close_check_pending
            || self.user_action_pending
            || self.dialog.is_some()
            || self.navigation.epoch() != expected_epoch
        {
            return Err(LaunchError::Busy);
        }
        self.accept_snapshot(snapshot, cx);
        let snapshot = self.snapshot.as_ref().ok_or(LaunchError::Unavailable)?;
        select_target(&mut self.navigation, snapshot, target)?;
        if activate {
            window.activate_window();
        }
        if matches!(target, LaunchTarget::Session { .. }) {
            self.settings = false;
            self.pending_recent = None;
            if !self.is_local_source() {
                self.select_local_source(window, cx);
            }
            self.ensure_terminal(window, cx);
        }
        cx.notify();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_manager::InitialSelection;
    use shellfox_core::{Preferences, ProfileId, Selection, Session, Tab, TabState};
    fn session() -> Session {
        let id = SessionId::new();
        Session {
            id,
            title: "Launch fixture".into(),
            cwd: "/fixture".into(),
            created_at: "2025-01-01T00:00:00Z".into(),
            updated_at: "2025-01-01T00:00:00Z".into(),
            settled_at: None,
            pinned_at: None,
            env: vec![],
            tabs: vec![Tab {
                id: TabId::new(),
                session_id: id,
                title: "Tab".into(),
                use_process_title: false,
                cwd: "/fixture".into(),
                ordinal: 0,
                created_at: None,
                profile_id: ProfileId::default(),
                wsl_launch: None,
                state: TabState::Pending,
            }],
        }
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            revision: 1,
            sessions: vec![session(), session()],
            preferences: Preferences::default(),
            selection: Selection::default(),
        }
    }
    #[test]
    fn launch_changes_only_the_chosen_window_and_show_preserves_navigation() {
        let snapshot = snapshot();
        let mut chosen = WindowSelection::new(InitialSelection::Empty);
        let mut other = WindowSelection::new(InitialSelection::Empty);
        chosen.accept_snapshot(&snapshot);
        other.accept_snapshot(&snapshot);
        let before = other.selected().clone();
        let epoch = other.epoch();
        let target = LaunchTarget::Session {
            session: snapshot.sessions[1].id,
            tab: None,
        };
        select_target(&mut chosen, &snapshot, &target).unwrap();
        assert_eq!(chosen.selected().session_id, Some(snapshot.sessions[1].id));
        assert_eq!(*other.selected(), before);
        assert_eq!(other.epoch(), epoch);
        let selected = chosen.selected().clone();
        let epoch = chosen.epoch();
        select_target(&mut chosen, &snapshot, &LaunchTarget::Show {}).unwrap();
        assert_eq!(*chosen.selected(), selected);
        assert_eq!(chosen.epoch(), epoch);
    }
    #[test]
    fn cold_launch_selects_the_requested_tab_before_attachment() {
        let snapshot = snapshot();
        let mut navigation = WindowSelection::new(InitialSelection::Empty);
        navigation.accept_snapshot(&snapshot);
        let session = &snapshot.sessions[1];
        let tab = session.tabs[0].id;
        let target = LaunchTarget::Session { session: session.id, tab: Some(tab) };
        select_target(&mut navigation, &snapshot, &target).unwrap();
        assert_eq!(navigation.selected().tab_id, Some(tab));
        assert_eq!(navigation.selected().session_id, Some(session.id));
    }

    #[test]
    fn missing_or_wrong_session_tab_is_rejected_without_retargeting() {
        let snapshot = snapshot();
        let mut navigation = WindowSelection::new(InitialSelection::Empty);
        navigation.accept_snapshot(&snapshot);
        let epoch = navigation.epoch();
        for target in [
            LaunchTarget::Session {
                session: SessionId::new(),
                tab: None,
            },
            LaunchTarget::Session {
                session: snapshot.sessions[0].id,
                tab: Some(snapshot.sessions[1].tabs[0].id),
            },
        ] {
            assert_eq!(
                select_target(&mut navigation, &snapshot, &target),
                Err(LaunchError::NotFound)
            );
            assert_eq!(navigation.epoch(), epoch);
            assert_eq!(navigation.selected(), &Selection::default());
        }
    }
}
