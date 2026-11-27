use crate::{
    activity::integration::{read_source, StartupEvidence, TerminalSourceFacts},
    activity::{tab_agent_state, tab_status, AgentState, TabFacts, TabLifecycle},
};
use shellfox_core::{Generation, TabId};
use shellfox_runtime::Phase;
use std::fmt;

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeTabActivity {
    pub tab: TabId,
    pub generation: Generation,
    pub dot: AgentState,
    pub has_agents: bool,
    pub closed: bool,
    pub error: Option<String>,
    /// The live program's title, set only while the tab has no explicit name.
    /// Volatile: never persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_title: Option<String>,
    /// A view holds this terminal's control lease. When it turns false, a
    /// view showing the terminal takes control back without a click.
    #[serde(default)]
    pub controlled: bool,
}

impl fmt::Debug for NativeTabActivity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeTabActivity")
            .field("tab", &self.tab)
            .field("generation", &self.generation)
            .field("dot", &self.dot)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl From<&TerminalSourceFacts> for NativeTabActivity {
    fn from(source: &TerminalSourceFacts) -> Self {
        let error = match &source.startup {
            StartupEvidence::Failed {
                sanitized_error,
                acknowledged: false,
            } => Some(sanitized_error.message.clone()),
            _ => None,
        };
        let lifecycle = if source.phase == Phase::Active
            && matches!(source.startup, StartupEvidence::Confirmed)
        {
            TabLifecycle::Open
        } else {
            TabLifecycle::Closed
        };
        let agents = source
            .observation
            .as_ref()
            .map_or(0, |observation| observation.agents);
        Self {
            tab: source.identity.tab_id,
            generation: source.identity.generation,
            dot: tab_agent_state(&TabFacts {
                lifecycle,
                status: tab_status(lifecycle, error.is_some(), source.observation.as_ref()),
                agents,
                busy: source.busy && source.activity_available,
                error: error.is_some(),
            }),
            has_agents: agents > 0,
            closed: source.phase == Phase::Closed,
            error,
            process_title: None,
            controlled: false,
        }
    }
}
impl crate::Service {
    fn local_administrator_startup_error(
        &self,
        identity: shellfox_runtime::Identity,
    ) -> Option<String> {
        let terminal = self.terminals.get(&identity.tab_id)?;
        if terminal.runtime.identity() != identity {
            return None;
        }
        let error = terminal.startup.as_ref()?.as_ref().err()?;
        match error.code {
            shellfox_runtime::ErrorCode::AdministratorStartup(detail) => Some(detail.to_string()),
            _ => None,
        }
    }

    pub(crate) fn desktop_tab_activity(&self) -> Vec<NativeTabActivity> {
        read_source(&self.source_facts_watch())
            .into_iter()
            .flat_map(|source| {
                let following: std::collections::HashSet<_> = source
                    .snapshot
                    .sessions
                    .iter()
                    .flat_map(|session| &session.tabs)
                    .filter_map(|tab| tab.use_process_title.then_some(tab.id))
                    .collect();
                source
                    .terminals
                    .iter()
                    .map(|terminal| {
                        let mut activity = NativeTabActivity::from(terminal);
                        activity.controlled = self.controlled(activity.tab);
                        // The public facts keep generic startup errors. Local desktop
                        // diagnostics can display the launcher's fixed error codes.
                        if activity.error.is_some() {
                            if let Some(detail) =
                                self.local_administrator_startup_error(terminal.identity)
                            {
                                activity.error = Some(detail);
                            }
                        }
                        if following.contains(&activity.tab) {
                            activity.process_title = terminal.title.clone();
                        }
                        activity
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}
