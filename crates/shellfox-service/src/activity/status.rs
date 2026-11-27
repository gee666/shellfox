use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootState {
    Alive,
    Exited,
    Unavailable,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    Healthy,
    Unknown,
}
/// Sanitized status only. No process paths, arguments, identities or environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    pub root: RootState,
    pub health: Health,
    pub agents: u32,
    pub reason: Option<String>,
    pub observed_at_ms: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Waiting,
    Error,
    Running,
    Unknown,
    Settled,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabStatus {
    Waiting,
    Error,
    Running,
    Unknown,
}
impl From<TabStatus> for Status {
    fn from(value: TabStatus) -> Self {
        match value {
            TabStatus::Waiting => Self::Waiting,
            TabStatus::Error => Self::Error,
            TabStatus::Running => Self::Running,
            TabStatus::Unknown => Self::Unknown,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabLifecycle {
    Launching,
    Open,
    Closed,
    LaunchUncertain,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Error,
    Busy,
    Running,
    Shell,
    Settled,
}
#[derive(Clone, Copy, Debug)]
pub struct TabFacts {
    pub lifecycle: TabLifecycle,
    pub status: TabStatus,
    pub agents: u32,
    pub busy: bool,
    pub error: bool,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Counts {
    pub running: u32,
    pub waiting: u32,
    pub unknown: u32,
    pub error: u32,
    pub closed: u32,
    pub agents: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionFacts {
    pub status: Status,
    pub activity_status: TabStatus,
    pub counts: Counts,
    pub agent_state: AgentState,
}
pub fn tab_status(
    lifecycle: TabLifecycle,
    error: bool,
    observation: Option<&Observation>,
) -> TabStatus {
    if error {
        return TabStatus::Error;
    }
    if lifecycle != TabLifecycle::Open {
        return TabStatus::Unknown;
    }
    match observation {
        Some(observation) if observation.root == RootState::Alive => {
            if observation.agents > 0 {
                TabStatus::Running
            } else if observation.health == Health::Healthy {
                TabStatus::Waiting
            } else {
                TabStatus::Unknown
            }
        }
        _ => TabStatus::Unknown,
    }
}
pub fn tab_agent_state(tab: &TabFacts) -> AgentState {
    if tab.error || tab.status == TabStatus::Error {
        AgentState::Error
    } else if tab.status == TabStatus::Running {
        if tab.busy {
            AgentState::Busy
        } else {
            AgentState::Running
        }
    } else {
        AgentState::Shell
    }
}
pub fn session_facts(tabs: &[TabFacts], settled: bool, error: bool) -> SessionFacts {
    let live: Vec<_> = tabs
        .iter()
        .filter(|tab| tab.lifecycle != TabLifecycle::Closed)
        .collect();
    let activity_status = if error || tabs.iter().any(|tab| tab.status == TabStatus::Error) {
        TabStatus::Error
    } else if live.iter().any(|tab| tab.status == TabStatus::Running) {
        TabStatus::Running
    } else if live.is_empty() || live.iter().any(|tab| tab.status == TabStatus::Unknown) {
        TabStatus::Unknown
    } else {
        TabStatus::Waiting
    };
    let mut counts = Counts::default();
    for tab in tabs {
        if tab.lifecycle == TabLifecycle::Closed {
            counts.closed += 1;
        }
        if tab.lifecycle != TabLifecycle::Closed || tab.status == TabStatus::Error {
            match tab.status {
                TabStatus::Running => counts.running += 1,
                TabStatus::Waiting => counts.waiting += 1,
                TabStatus::Unknown => counts.unknown += 1,
                TabStatus::Error => counts.error += 1,
            }
        }
        if tab.lifecycle != TabLifecycle::Closed {
            counts.agents = counts.agents.saturating_add(tab.agents);
        }
    }
    let agent_state = if settled {
        AgentState::Settled
    } else if error {
        AgentState::Error
    } else {
        live.iter()
            .map(|tab| tab_agent_state(tab))
            .max_by_key(|state| match state {
                AgentState::Error => 3,
                AgentState::Busy => 2,
                AgentState::Running => 1,
                AgentState::Shell => 0,
                AgentState::Settled => -1,
            })
            .unwrap_or(AgentState::Shell)
    };
    SessionFacts {
        status: if settled {
            Status::Settled
        } else {
            activity_status.into()
        },
        activity_status,
        counts,
        agent_state,
    }
}

impl From<shellfox_protocol::Status> for Status {
    fn from(value: shellfox_protocol::Status) -> Self {
        use shellfox_protocol::Status as Wire;
        match value {
            Wire::Waiting => Self::Waiting,
            Wire::Error => Self::Error,
            Wire::Running => Self::Running,
            Wire::Unknown => Self::Unknown,
            Wire::Settled => Self::Settled,
        }
    }
}
/// Sort rank of a reported status (0.9 `compareSessions`). Empty sessions and
/// ordinary shells share the blue rank.
pub fn status_rank(status: Status) -> u8 {
    match status {
        Status::Running => 0,
        Status::Error => 1,
        Status::Waiting | Status::Unknown => 2,
        Status::Settled => 3,
    }
}
/// Sort rank of the visible dot (0.9 `sortVisibleSessions`).
pub fn agent_state_rank(state: AgentState) -> u8 {
    match state {
        AgentState::Error => 0,
        AgentState::Running => 1,
        AgentState::Busy => 2,
        AgentState::Shell => 3,
        AgentState::Settled => 4,
    }
}
