use crate::*;
use serde::{Deserialize, Serialize};

/// Metadata-only wire boundary. Runtime ownership/cleanup cannot be invoked by
/// remote clients. Existing wire method names and camelCase fields are retained.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "payload", deny_unknown_fields)]
pub enum HostRequest {
    #[serde(rename = "getSnapshot")]
    GetSnapshot {},
    #[serde(rename = "createSession")]
    CreateSession(CreateSession),
    #[serde(rename = "addTab")]
    AddTab {
        #[serde(rename = "sessionId")]
        session_id: SessionId,
        #[serde(flatten)]
        input: AddTab,
    },
    #[serde(rename = "renameSession")]
    RenameSession {
        #[serde(rename = "sessionId")]
        session_id: SessionId,
        title: String,
    },
    #[serde(rename = "renameTab")]
    RenameTab {
        #[serde(rename = "sessionId")]
        session_id: SessionId,
        #[serde(rename = "tabId")]
        tab_id: TabId,
        title: String,
    },
    #[serde(rename = "reorderTabs")]
    ReorderTabs {
        #[serde(rename = "sessionId")]
        session_id: SessionId,
        #[serde(rename = "tabIds")]
        tab_ids: Vec<TabId>,
    },
    #[serde(rename = "setSessionPinned")]
    SetSessionPinned {
        #[serde(rename = "sessionId")]
        session_id: SessionId,
        pinned: bool,
    },
    #[serde(rename = "activateSession")]
    ActivateSession {
        #[serde(rename = "sessionId")]
        session_id: SessionId,
    },
    #[serde(rename = "saveSettings")]
    SaveSettings {
        #[serde(flatten)]
        preferences: PreferencesPatch,
        #[serde(rename = "terminalEnv")]
        terminal_env: Option<Vec<EnvVar>>,
    },
    #[serde(rename = "setSessionEnv")]
    SetSessionEnv {
        #[serde(rename = "sessionId")]
        session_id: SessionId,
        env: Vec<EnvVar>,
    },
    #[serde(rename = "setTerminalEnv")]
    SetTerminalEnv { env: Vec<EnvVar> },
}
impl Host {
    pub fn request(&self, request: HostRequest) -> Result<Snapshot> {
        match request {
            HostRequest::GetSnapshot {} => self.snapshot(),
            HostRequest::CreateSession(input) => self.create_session(input),
            HostRequest::AddTab { session_id, input } => self.add_tab(session_id, input),
            HostRequest::RenameSession { session_id, title } => {
                self.rename_session(session_id, &title)
            }
            HostRequest::RenameTab {
                session_id,
                tab_id,
                title,
            } => {
                if !self
                    .snapshot()?
                    .sessions
                    .iter()
                    .any(|s| s.id == session_id && s.tabs.iter().any(|t| t.id == tab_id))
                {
                    return Err(Error::new(
                        ErrorCode::NotFound,
                        "Tab does not belong to this session.",
                    ));
                }
                self.rename_tab(tab_id, &title)
            }
            HostRequest::ReorderTabs {
                session_id,
                tab_ids,
            } => self.reorder_tabs(session_id, &tab_ids),
            HostRequest::SetSessionPinned { session_id, pinned } => {
                self.set_session_pinned(session_id, pinned)
            }
            HostRequest::ActivateSession { session_id } => self.select_session(session_id),
            HostRequest::SetSessionEnv { session_id, env } => self.set_session_env(session_id, env),
            HostRequest::SetTerminalEnv { env } => self.set_terminal_env(env),
            HostRequest::SaveSettings {
                preferences,
                terminal_env,
            } => self.save_settings(preferences, terminal_env),
        }
    }
    /// Omitted global environment preserves the latest saved value atomically.
    pub fn save_settings(
        &self,
        preferences: PreferencesPatch,
        terminal_env: Option<Vec<EnvVar>>,
    ) -> Result<Snapshot> {
        let terminal_env = terminal_env
            .map(|env| normalize_env(env, cfg!(windows)))
            .transpose()?;
        self.update(ChangeReason::Settings, move |data, _| {
            let preferences = data.state.preferences.patched(preferences)?;
            let mut changed = preferences != data.state.preferences;
            data.state.preferences = preferences;
            if let Some(env) = terminal_env {
                changed |= env != data.state.terminal_env;
                data.state.terminal_env = env;
            }
            Ok(changed)
        })
    }
}
