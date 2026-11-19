use crate::{
    activity::{AgentState, TabFacts, TabLifecycle, tab_agent_state, tab_status},
    activity_integration::{StartupEvidence, TerminalSourceFacts, read_source},
};
use shellfox_core::{Generation, TabId};
use shellfox_runtime::{Phase, SourceKind};
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
    /// Volatile title for the local desktop only; not host facts or persisted state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_title: Option<String>,
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
        }
    }
}
impl crate::Service {
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
                        if following.contains(&activity.tab) {
                            activity.process_title = self
                                .terminals
                                .get(&activity.tab)
                                .map(|terminal| &terminal.runtime)
                                .filter(|runtime| {
                                    runtime.source_kind() == SourceKind::OwnedPty
                                        && runtime.phase() == Phase::Active
                                        && runtime.identity() == terminal.identity
                                })
                                .and_then(|runtime| runtime.metadata())
                                .and_then(|metadata| {
                                    metadata.title.as_deref().and_then(|title| {
                                        display_title(title, conpty_owner_image())
                                    })
                                });
                        }
                        activity
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

fn conpty_owner_image() -> Option<&'static str> {
    #[cfg(windows)]
    {
        static IMAGE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        IMAGE
            .get_or_init(|| {
                std::env::current_exe()
                    .ok()?
                    .into_os_string()
                    .into_string()
                    .ok()
            })
            .as_deref()
    }
    #[cfg(not(windows))]
    None
}

fn display_title(title: &str, conpty_owner_image: Option<&str>) -> Option<String> {
    let title = title.trim();
    // System ConPTY can announce the owner's executable as its initial title.
    // It is not a shell title. Explicit tab names never go through this policy.
    if conpty_owner_image.is_some_and(|image| {
        windows_image_path(title).eq_ignore_ascii_case(windows_image_path(image))
    }) {
        return None;
    }
    shellfox_core::validation::title(title)
        .ok()
        .map(|()| title.to_owned())
}

fn windows_image_path(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_display_titles_reject_blank_controls_and_oversized_content() {
        assert_eq!(super::display_title(" Work ", None), Some("Work".into()));
        for title in ["  ", "one\ntwo", &"x".repeat(201)] {
            assert_eq!(super::display_title(title, None), None);
        }
    }

    #[test]
    fn conpty_owner_title_keeps_creation_label_without_hiding_application_titles() {
        let image = r"C:\Apps\Shellfox\shellfox.exe";
        let display = |title| super::display_title(title, Some(image));
        assert_eq!(display(image), None);
        assert_eq!(display(r"\\?\c:\apps\shellfox\SHELLFOX.EXE"), None);
        assert_eq!(display("Build work").as_deref(), Some("Build work"));
        assert_eq!(
            display(r"C:\Projects\work").as_deref(),
            Some(r"C:\Projects\work")
        );
        assert_eq!(super::display_title(image, None).as_deref(), Some(image));
    }
}
