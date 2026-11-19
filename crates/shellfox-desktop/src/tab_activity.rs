use crate::{host_client::HostClient, root::Root};
use gpui::{Context, Task, Window};

#[path = "tab_activity_dot.rs"]
mod dot;
pub(crate) use dot::status_dot;
use shellfox_core::{Session, Tab, TabId, TabState, TerminalIdentity};
use shellfox_service::{
    NativeTabActivity,
    activity::AgentState,
    activity_integration::read_source,
    native::{NativeAction, NativeResponse},
};
use std::{collections::HashMap, time::Duration};

#[derive(Default)]
pub(crate) struct TabActivity {
    states: HashMap<TabId, NativeTabActivity>,
    task: Option<Task<()>>,
}

impl Root {
    pub(crate) fn watch_tab_activity(&mut self, client: HostClient, window: &Window, cx: &mut Context<Self>) {
        self.tab_activity.task = Some(cx.spawn_in(window, async move |root, cx| {
            let mut watch = if client.connected_to_headless() { None } else { client.source_facts_watch().await.ok() };
            loop {
                let page_size = watch.as_mut().and_then(|watch| {
                    watch.borrow_and_update();
                    read_source(watch).map(|source| source.settings.history_page_size as usize)
                });
                let states = match client.execute(NativeAction::ReadTabActivity).await {
                    Ok(NativeResponse::TabActivity(states)) => Some(states.into_iter().map(|state| (state.tab, state)).collect()),
                    _ => None,
                };
                if root.update_in(cx, |root, window, cx| {
                    if page_size.is_some_and(|size| root.apply_history_page_size(size)) { cx.notify(); }
                    if let Some(states) = states { root.accept_tab_activity(states, window, cx); }
                    !root.closing_window
                }).ok() != Some(true) { return; }
                if watch.as_ref().is_some_and(|watch| watch.has_changed().is_err()) { break; }
                cx.background_executor().timer(Duration::from_millis(250)).await;
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
        if self.failures.contains_key(&tab.id)
            || self
                .views
                .get(&tab.id)
                .is_some_and(|view| view.read(cx).is_failed())
        {
            AgentState::Error
        } else {
            self.tab_activity
                .states
                .get(&tab.id)
                .map_or(AgentState::Shell, |state| state.dot)
        }
    }

    pub(crate) fn tab_status_label(&self, id: TabId, state: AgentState) -> String {
        match state {
            AgentState::Shell => "Shell".into(),
            AgentState::Running => "Agent idle".into(),
            AgentState::Busy => "Agent working".into(),
            AgentState::Settled => "Settled".into(),
            AgentState::Error => self.failures.get(&id).map(|error| error.message.as_str())
                .or_else(|| self.tab_activity.states.get(&id)?.error.as_deref())
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
            .find_map(|tab| {
                let state = self.tab_activity.states.get(&tab.id);
                self.failures
                    .get(&tab.id)
                    .map(|error| error.message.clone())
                    .or_else(|| state.and_then(|state| state.error.clone()))
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
