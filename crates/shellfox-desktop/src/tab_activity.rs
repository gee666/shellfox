use crate::{host_client::HostClient, root::Root};
use gpui::{Context, Task, Window};

#[path = "tab_activity_dot.rs"]
mod dot;
pub(crate) use dot::status_dot;
use shellfox_core::{Session, Tab, TabId, TabState, TerminalIdentity};
use shellfox_service::{
    NativeTabActivity,
    activity::AgentState,
};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct TabActivity {
    states: HashMap<TabId, NativeTabActivity>,
    task: Option<Task<()>>,
}

impl Root {
    pub(crate) fn watch_tab_activity(&mut self, client: HostClient, window: &Window, cx: &mut Context<Self>) {
        self.tab_activity.task = Some(cx.spawn_in(window, async move |root, cx| {
            let mut changes = client.tab_activity_changes();
            while let Ok(states) = changes.next().await {
                let states = states.iter().map(|state| (state.tab, state.clone())).collect();
                if root.update_in(cx, |root, window, cx| {
                    root.accept_tab_activity(states, window, cx);
                    !root.closing_window
                }).ok() != Some(true) { return; }
            }
            let _ = root.update_in(cx, |root, _, cx| {
                root.tab_activity.states.clear();
                cx.notify();
            });
        }));
    }

    fn accept_tab_activity(&mut self, states: HashMap<TabId, NativeTabActivity>, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_activity.states == states { return; }
        let previous = self.selected_tab();
        self.tab_activity.states = states;
        if previous.is_some() && previous != self.selected_tab() {
            self.ensure_terminal(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn local_tab_viewable(&self, tab: &Tab) -> bool {
        tab_is_viewable(tab.state, tab.profile_id.as_str(), self.tab_activity.states.get(&tab.id))
    }

    pub(crate) fn display_tab_title<'a>(&'a self, tab: &'a Tab) -> &'a str {
        if tab.use_process_title
            && let Some(title) = self.tab_activity.states.get(&tab.id).and_then(|state| state.process_title.as_deref())
        {
            return title;
        }
        &tab.title
    }

    pub(crate) fn tab_agent_state(&self, tab: &Tab, cx: &gpui::App) -> AgentState {
        let view_failed = self.failures.contains_key(&tab.id)
            || self
                .views
                .get(&tab.id)
                .is_some_and(|view| view.read(cx).is_failed());
        local_dot(self.tab_activity.states.get(&tab.id), view_failed)
    }

    pub(crate) fn tab_status_label(&self, id: TabId, state: AgentState) -> String {
        match state {
            AgentState::Shell => "Shell".into(),
            AgentState::Running => "Agent idle".into(),
            AgentState::Busy => "Agent working".into(),
            AgentState::Settled => "Settled".into(),
            AgentState::Error => self.local_tab_error_text(id)
                .map_or_else(|| "Error".into(), |message| format!("Error: {message}")),
        }
    }

    pub(crate) fn tab_has_agents(&self, id: TabId) -> bool {
        self.tab_activity
            .states
            .get(&id)
            .is_some_and(|state| state.has_agents)
    }

    pub(crate) fn tab_close_identity(&self, id: TabId) -> Option<TerminalIdentity> {
        self.host.as_ref()?;
        let activity = self
            .tab_activity
            .states
            .get(&id)
            .filter(|activity| !activity.closed)?;
        Some(TerminalIdentity {
            tab_id: activity.tab,
            generation: activity.generation,
        })
    }

    pub(crate) fn session_error_text(&self, session: &Session) -> Option<String> {
        // A failed launch belongs to the session even after the attempted
        // runtime closes. Archived historical tabs aren't new launch errors.
        session
            .tabs
            .iter()
            .filter(|tab| tab.state != TabState::Archived)
            .find_map(|tab| self.local_tab_error_text(tab.id))
            .map(str::to_owned)
    }

    fn local_tab_error_text(&self, id: TabId) -> Option<&str> {
        let host = self.tab_activity.states.get(&id);
        host.and_then(|state| state.error.as_deref()).or_else(|| {
            if active_agent(host) {
                return None;
            }
            self.failures.get(&id).map(|error| error.message.as_str())
        })
    }

    pub(crate) fn session_has_error(&self, session: &Session, cx: &gpui::App) -> bool {
        session
            .tabs
            .iter()
            .any(|tab| self.tab_agent_state(tab, cx) == AgentState::Error)
    }

    pub(crate) fn session_agent_state(&self, session: &Session, cx: &gpui::App) -> AgentState {
        if session.settled_at.is_some() {
            return AgentState::Settled;
        }
        if self.session_error_text(session).is_some() {
            return AgentState::Error;
        }
        session
            .tabs
            .iter()
            .filter(|tab| {
                tab.state != TabState::Archived
                    && !self
                        .tab_activity
                        .states
                        .get(&tab.id)
                        .is_some_and(|state| state.closed)
            })
            .map(|tab| self.tab_agent_state(tab, cx))
            .max_by_key(|state| match state {
                AgentState::Error => 3,
                AgentState::Busy => 2,
                AgentState::Running => 1,
                AgentState::Shell => 0,
                AgentState::Settled => -1,
            })
            .unwrap_or(AgentState::Shell)
    }
}

fn active_agent(activity: Option<&NativeTabActivity>) -> bool {
    activity.is_some_and(|state| {
        !state.closed && state.error.is_none() && matches!(state.dot, AgentState::Running | AgentState::Busy)
    })
}

fn local_dot(activity: Option<&NativeTabActivity>, view_failed: bool) -> AgentState {
    // An attachment can fail while the host still owns a live agent. The view
    // handles reconnect separately; its failure isn't the agent's status.
    if view_failed && !active_agent(activity) {
        AgentState::Error
    } else {
        activity.map_or(AgentState::Shell, |state| state.dot)
    }
}

fn tab_is_viewable(state: TabState, profile: &str, activity: Option<&NativeTabActivity>) -> bool {
    state != TabState::Archived
        && (profile == shellfox_service::AUTOMATION_PROFILE_ID
            || activity.is_none_or(|activity| !activity.closed || activity.error.is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_runtime::{Identity, Phase};
    use shellfox_service::{
        activity::{Health, Observation, RootState},
        activity_integration::{StartupEvidence, TerminalSourceFacts},
    };

    #[test]
    fn closed_shell_is_hidden_but_failed_start_and_completed_automation_remain_visible() {
        let mut closed = NativeTabActivity {
            tab: TabId::new(),
            generation: shellfox_core::Generation::new(),
            dot: AgentState::Shell,
            has_agents: false,
            closed: true,
            error: None,
            process_title: None,
        };
        assert!(!tab_is_viewable(TabState::Pending, "pwsh", Some(&closed)));
        assert!(tab_is_viewable(TabState::Pending, "pwsh", None));
        assert!(tab_is_viewable(TabState::Pending, "automation-pipe", Some(&closed)));
        assert!(!tab_is_viewable(TabState::Archived, "automation-pipe", Some(&closed)));
        closed.error = Some("Terminal failed to start.".into());
        assert!(tab_is_viewable(TabState::Pending, "pwsh", Some(&closed)));
        assert!(!tab_is_viewable(TabState::Archived, "pwsh", Some(&closed)));
    }

    #[test]
    fn a_view_failure_does_not_override_live_agent_status() {
        let mut live = NativeTabActivity {
            tab: TabId::new(),
            generation: shellfox_core::Generation::new(),
            dot: AgentState::Running,
            has_agents: true,
            closed: false,
            error: None,
            process_title: None,
        };
        assert_eq!(local_dot(Some(&live), true), AgentState::Running);
        live.dot = AgentState::Busy;
        assert_eq!(local_dot(Some(&live), true), AgentState::Busy);
        live.closed = true;
        assert_eq!(local_dot(Some(&live), true), AgentState::Error);
        live.dot = AgentState::Error;
        live.error = Some("Terminal startup failed.".into());
        assert_eq!(local_dot(Some(&live), false), AgentState::Error);
        assert_eq!(local_dot(None, true), AgentState::Error);
    }

    #[test]
    fn an_active_shell_pulses_only_for_detected_active_agents() {
        let mut source = TerminalSourceFacts {
            identity: Identity {
                session_id: Default::default(),
                tab_id: Default::default(),
                generation: Default::default(),
            },
            phase: Phase::Active,
            startup: StartupEvidence::Confirmed,
            source_kind: None,
            automation_exit_code: None,
            observation: Some(Observation {
                root: RootState::Alive,
                agents: 0,
                health: Health::Healthy,
                reason: None,
                observed_at_ms: 0,
            }),
            busy: true,
            activity_available: true,
        };
        assert_eq!(NativeTabActivity::from(&source).dot, AgentState::Shell);
        assert!(!NativeTabActivity::from(&source).has_agents);
        source.observation.as_mut().unwrap().agents = 1;
        assert_eq!(NativeTabActivity::from(&source).dot, AgentState::Busy);
        source.busy = false;
        assert_eq!(NativeTabActivity::from(&source).dot, AgentState::Running);
        source.busy = true;
        source.activity_available = false;
        assert_eq!(NativeTabActivity::from(&source).dot, AgentState::Running);
        assert!(NativeTabActivity::from(&source).has_agents);
        source.phase = Phase::Closed;
        assert_eq!(NativeTabActivity::from(&source).dot, AgentState::Shell);
        source.startup = StartupEvidence::Failed {
            sanitized_error: shellfox_protocol::AppError {
                code: shellfox_protocol::ErrorCode::LaunchFailed,
                message: "Owned terminal startup failed.".into(),
                retryable: true,
            },
            acknowledged: false,
        };
        assert_eq!(NativeTabActivity::from(&source).dot, AgentState::Error);
    }
}
