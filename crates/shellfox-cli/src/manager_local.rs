//! Manager methods on the local host, built on the operations the desktop uses.
//! Directory helpers run here: the CLI is the same user on the same machine.
use crate::{manager::Call, output::CliError, session_view};
use anyhow::Result;
use serde_json::{json, Value};
use shellfox_core::{SessionId, Snapshot, TabId};
use shellfox_host::HostRequest;
use shellfox_protocol::RemoteCommand as R;
use shellfox_service::{
    local::{DesktopReply, DesktopRequest, LocalClient},
    local_api::{LocalApiRequest, LocalApiResponse, SettingsRequest},
    NativeTabActivity,
};
use std::sync::Mutex;

/// One command's view of the local host. The host holds a client's repeated
/// activity read until the activity changes (the desktop's feed), so a command
/// reads it once and reuses that answer.
pub struct Local<'a>(pub &'a LocalClient, Mutex<Option<Vec<NativeTabActivity>>>);

fn not_found(message: &str) -> anyhow::Error {
    CliError::coded("NOT_FOUND", message, false)
}
fn unexpected() -> anyhow::Error {
    anyhow::anyhow!("Unexpected host response.")
}

impl<'a> Local<'a> {
    pub fn new(client: &'a LocalClient) -> Self {
        Self(client, Mutex::default())
    }
    pub async fn snapshot(&self) -> Result<Snapshot> {
        self.local_api(LocalApiRequest::Snapshot {}).await
    }
    async fn local_api(&self, request: LocalApiRequest) -> Result<Snapshot> {
        match self.0.execute(request).await? {
            LocalApiResponse::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(unexpected()),
        }
    }
    async fn desktop(&self, request: DesktopRequest) -> Result<DesktopReply> {
        self.0.request(&request).await
    }
    async fn view(&self, request: DesktopRequest) -> Result<Snapshot> {
        match self.desktop(request).await? {
            DesktopReply::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(unexpected()),
        }
    }
    async fn metadata(&self, request: HostRequest) -> Result<Snapshot> {
        self.view(DesktopRequest::Metadata(request)).await
    }
    pub async fn activity(&self) -> Result<Vec<NativeTabActivity>> {
        let cached = || self.1.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(activity) = cached().as_ref() {
            return Ok(activity.clone());
        }
        let DesktopReply::TabActivity(activity) =
            self.desktop(DesktopRequest::ReadTabActivity).await?
        else {
            return Err(unexpected());
        };
        *cached() = Some(activity.clone());
        Ok(activity)
    }
    async fn settings(&self, request: SettingsRequest) -> Result<Value> {
        match self
            .0
            .execute(LocalApiRequest::Settings { request })
            .await?
        {
            LocalApiResponse::Settings(settings) => {
                let saved = self.snapshot().await?.preferences.default_profile_id;
                Ok(settings_record(
                    serde_json::to_value(settings)?,
                    saved.as_str(),
                ))
            }
            _ => Err(unexpected()),
        }
    }
    /// One session from `snapshot`, with live status.
    async fn session(&self, snapshot: &Snapshot, id: SessionId) -> Result<Value> {
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| not_found("Session not found."))?;
        let mut records = self.records_of(std::slice::from_ref(session)).await?;
        records.pop().ok_or_else(|| not_found("Session not found."))
    }
    async fn records(&self, snapshot: &Snapshot) -> Result<Vec<Value>> {
        self.records_of(&snapshot.sessions).await
    }
    /// The host's 0.9 v1 records, as a peer reads them. A session the v1 shape
    /// can't represent, or a host from before SessionRecords, keeps the
    /// local record.
    async fn records_of(&self, sessions: &[shellfox_core::Session]) -> Result<Vec<Value>> {
        let records = match self.desktop(DesktopRequest::SessionRecords).await {
            Ok(DesktopReply::SessionRecords(records)) => records,
            _ => Vec::new(),
        };
        let mut out = Vec::with_capacity(sessions.len());
        for session in sessions {
            out.push(match records.iter().find(|r| r.id == session.id) {
                Some(record) => serde_json::to_value(record)?,
                None => session_view::record(session, &self.activity().await?),
            });
        }
        Ok(out)
    }
    /// The open terminal's current generation, or None when it isn't running.
    pub async fn live(&self, tab: TabId) -> Result<Option<shellfox_core::Generation>> {
        Ok(self
            .activity()
            .await?
            .into_iter()
            .find(|a| a.tab == tab && !a.closed)
            .map(|a| a.generation))
    }
    /// The profile for a terminal in `cwd`; see shellfox_service::folder_profile.
    async fn folder_profile(
        &self,
        snapshot: &Snapshot,
        cwd: &str,
    ) -> Result<shellfox_core::ProfileId> {
        let saved = snapshot.preferences.default_profile_id.clone();
        let DesktopReply::TerminalProfiles(catalog) =
            self.desktop(DesktopRequest::TerminalProfiles).await?
        else {
            return Err(unexpected());
        };
        let candidates: Vec<shellfox_service::LaunchCandidate> =
            catalog.profiles.profiles.iter().map(Into::into).collect();
        shellfox_service::folder_profile(&candidates, saved.as_str(), cwd, cfg!(windows))
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| {
                CliError::coded(
                    "UNSUPPORTED",
                    "No terminal profile is available for this folder.",
                    false,
                )
            })
    }
    /// As in 0.9, a saved tab whose shell isn't running still closes; a running
    /// one only at `generation`, when given.
    pub(crate) async fn close_tab(
        &self,
        tab: TabId,
        generation: Option<shellfox_core::Generation>,
    ) -> Result<Value> {
        let live = self.live(tab).await?;
        if live.zip(generation).is_some_and(|(live, given)| live != given) {
            return Err(CliError::coded(
                "TARGET_LOST",
                "This terminal generation is stale.",
                false,
            ));
        }
        let before = self.snapshot().await?;
        let session = before
            .sessions
            .iter()
            .find(|s| s.tabs.iter().any(|t| t.id == tab))
            .map(|s| s.id)
            .ok_or_else(|| not_found("Tab not found."))?;
        let after = self.local_api(LocalApiRequest::CloseTab { id: tab }).await;
        self.after(session, after).await
    }
    /// The session a createSession request made, from the host's receipt.
    async fn created(&self, request_id: SessionId) -> Result<SessionId> {
        match self
            .desktop(DesktopRequest::CreationRequestReceipt { request_id })
            .await?
        {
            DesktopReply::CreationRequestReceipt {
                request_id: id,
                receipt: shellfox_host::CreationRequestReceipt::SessionCreated { session_id },
            } if id == request_id => Ok(session_id),
            DesktopReply::CreationRequestReceipt { .. } => {
                anyhow::bail!("The host did not return the new session.")
            }
            _ => Err(unexpected()),
        }
    }
    async fn after(&self, id: SessionId, snapshot: Result<Snapshot>) -> Result<Value> {
        self.session(&snapshot?, id).await
    }

    pub async fn call(&self, call: Call) -> Result<Value> {
        let command = match call {
            Call::Remote(command) => command,
            Call::Focus(id) => {
                let snapshot = self.snapshot().await?;
                let session = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or_else(|| not_found("Session not found."))?;
                let activity = self.activity().await?;
                if !session
                    .tabs
                    .iter()
                    .any(|t| activity.iter().any(|a| a.tab == t.id && !a.closed))
                {
                    return Err(CliError::coded(
                        "TARGET_LOST",
                        "No live embedded shell. Activate the session to open one.",
                        false,
                    ));
                }
                self.metadata(HostRequest::ActivateSession { session_id: id })
                    .await?;
                return Ok(json!({ "focused": true }));
            }
            Call::Refresh(id) => {
                let snapshot = self.snapshot().await?;
                return self.session(&snapshot, id).await;
            }
            Call::SaveSettings(patch) => return self.settings(SettingsRequest::Save(patch)).await,
            Call::ReadTerminal(input) => return self.read_terminal(input).await,
            Call::EnsureIntegration(kind) => {
                return crate::desktop_handoff::ensure_integration(kind)
            }
            Call::GetKeepAwake => {
                let report = self.0.power_status().await?;
                return Ok(
                    json!({ "active": report.status == shellfox_service::power::PowerStatus::Active }),
                );
            }
            Call::SetKeepAwake(active) => {
                let report = self.0.set_power(active).await?;
                if report.pending || report.error.is_some() {
                    return Err(CliError::coded(
                        "NATIVE_UNAVAILABLE",
                        report
                            .error
                            .unwrap_or_else(|| "OS sleep inhibition failed.".into()),
                        false,
                    ));
                }
                return Ok(
                    json!({ "active": report.status == shellfox_service::power::PowerStatus::Active }),
                );
            }
        };
        match command {
            R::GetSnapshot(_) => {
                let snapshot = self.snapshot().await?;
                Ok(json!({
                    "revision": snapshot.revision,
                    "sessions": self.records(&snapshot).await?,
                    "settings": self.settings(SettingsRequest::Get {}).await?,
                }))
            }
            R::GetTerminalProfiles(_) => {
                match self.desktop(DesktopRequest::TerminalProfiles).await? {
                    DesktopReply::TerminalProfiles(catalog) => {
                        Ok(serde_json::to_value(&catalog.profiles)?)
                    }
                    _ => Err(unexpected()),
                }
            }
            R::GetHomeDirectory(_) => Ok(json!({ "cwd": home()? })),
            R::ResolveDirectory(input) => directory(&input.path, Directory::Resolve),
            R::CompleteDirectory(input) => directory(&input.path, Directory::Complete),
            R::CreateDirectory(input) => {
                let path = self
                    .file_path(input.session_id, input.tab_id, input.path)
                    .await?;
                directory(&path, Directory::Create)
            }
            R::CreateSession(input) => {
                let before = self.snapshot().await?;
                let profile_id = self.folder_profile(&before, &input.cwd).await?;
                let request = shellfox_core::CreateSession {
                    request_id: input.request_id,
                    title: input.title,
                    cwd: input.cwd,
                    profile_id,
                };
                let request_id = request.request_id;
                let after = self
                    .local_api(LocalApiRequest::CreateSession { input: request })
                    .await?;
                // The receipt names the session even when this was a retry.
                let created = self.created(request_id).await?;
                self.session(&after, created).await
            }
            R::AddTab(input) => {
                let before = self.snapshot().await?;
                let profile_id = match input.profile_id {
                    Some(profile) => profile
                        .parse()
                        .map_err(|_| CliError::usage("Invalid profile ID."))?,
                    None => {
                        let session = before
                            .sessions
                            .iter()
                            .find(|s| s.id == input.session_id)
                            .ok_or_else(|| not_found("Session not found."))?;
                        let cwd = input.cwd.as_deref().unwrap_or(&session.cwd);
                        self.folder_profile(&before, cwd).await?
                    }
                };
                let tab = shellfox_core::AddTab {
                    title: input.title,
                    cwd: input.cwd,
                    profile_id,
                };
                let after = self
                    .local_api(LocalApiRequest::AddTab {
                        id: input.session_id,
                        input: tab,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            R::ActivateSession(input) => {
                let after = self
                    .local_api(LocalApiRequest::ActivateSession {
                        id: input.session_id,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            R::RenameSession(input) => {
                let after = self
                    .local_api(LocalApiRequest::RenameSession {
                        id: input.session_id,
                        title: input.title,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            R::RenameTab(input) => {
                let request = HostRequest::RenameTab {
                    session_id: input.session_id,
                    tab_id: input.tab_id,
                    title: input.title,
                };
                let after = self.metadata(request).await;
                self.after(input.session_id, after).await
            }
            R::ReorderTabs(input) => {
                let after = self
                    .metadata(HostRequest::ReorderTabs {
                        session_id: input.session_id,
                        tab_ids: input.tab_ids,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            R::SetSessionPinned(input) => {
                let after = self
                    .metadata(HostRequest::SetSessionPinned {
                        session_id: input.session_id,
                        pinned: input.pinned,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            R::SetSessionEnv(input) => {
                let after = self
                    .metadata(HostRequest::SetSessionEnv {
                        session_id: input.session_id,
                        env: input.env,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            // As in 0.9, settling never closes terminals, so confirmActive changes nothing here.
            R::SettleSession(input) => {
                let after = self
                    .view(DesktopRequest::SetSettled {
                        session: input.session_id,
                        settled: true,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            R::UnsettleSession(input) => {
                let after = self
                    .view(DesktopRequest::SetSettled {
                        session: input.session_id,
                        settled: false,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            R::ClearSessionError(input) => {
                let after = self
                    .view(DesktopRequest::ClearSessionError {
                        session: input.session_id,
                    })
                    .await;
                self.after(input.session_id, after).await
            }
            R::DeleteSession(input) => {
                self.local_api(LocalApiRequest::DeleteSession {
                    id: input.session_id,
                })
                .await?;
                Ok(json!({ "deleted": true }))
            }
            R::SetTerminalEnv(input) => {
                self.metadata(HostRequest::SetTerminalEnv { env: input.env })
                    .await?;
                self.settings(SettingsRequest::Get {}).await
            }
            R::CloseTab(input) => self.close_tab(input.tab_id, Some(input.generation)).await,
            R::GetSessionStats(input) => {
                let snapshot = self.snapshot().await?;
                let mut sessions = self.records(&snapshot).await?;
                if let Some(id) = input.session_id {
                    sessions.retain(|s| s["id"] == id.to_string());
                    if sessions.is_empty() {
                        return Err(not_found("Session not found."));
                    }
                }
                let DesktopReply::TabStats(measured) =
                    self.desktop(DesktopRequest::TabStats).await?
                else {
                    return Err(unexpected());
                };
                let mut tabs = Vec::new();
                for session in &mut sessions {
                    session["env"] = json!([]);
                    for tab in session["tabs"].as_array().into_iter().flatten() {
                        let stat = measured
                            .iter()
                            .find(|stat| tab["id"] == stat.tab_id.to_string());
                        // Measured tabs have a running shell, even before the activity feed says so.
                        if let Some(stat) = stat {
                            tabs.push(serde_json::to_value(stat)?);
                        }
                    }
                }
                Ok(json!({ "sessions": sessions, "tabs": tabs }))
            }
            R::SetVisibleSessions(input) => {
                self.desktop(DesktopRequest::SetVisibleSessions(input))
                    .await?;
                Ok(json!({ "configured": true }))
            }
            R::ListFiles(input) => self.list_files(input).await,
            R::ReadFile(input) => self.read_file(input).await,
            R::WriteFile(input) => self.write_file(input).await,
            command => self.terminal_call(command).await,
        }
    }
}

/// 0.9's settings record: its key order, `pythonPath` (no longer used) and
/// the selected `terminalProfileId`.
fn settings_record(mut settings: Value, default_profile: &str) -> Value {
    let Some(fields) = settings.as_object_mut() else {
        return settings;
    };
    fields.entry("pythonPath").or_insert(Value::Null);
    // 0.9 wrote whole hours as integers.
    for key in ["sessionSettleHours", "sessionDeleteHours"] {
        if let Some(hours) = fields
            .get(key)
            .and_then(Value::as_f64)
            .filter(|h| h.fract() == 0.0)
        {
            fields.insert(key.into(), json!(hours as i64));
        }
    }
    if fields.get("terminalProfileId").is_none_or(Value::is_null) {
        fields.insert("terminalProfileId".into(), json!(default_profile));
    }
    const ORDER: [&str; 14] = [
        "version",
        "terminalEnv",
        "autoUpdate",
        "pythonPath",
        "accentColor",
        "backgroundColor",
        "adapterId",
        "shellId",
        "shellExecutable",
        "terminalProfileId",
        "processRules",
        "historyPageSize",
        "sessionSettleHours",
        "sessionDeleteHours",
    ];
    let mut ordered = serde_json::Map::new();
    for key in ORDER {
        if let Some(value) = fields.remove(key) {
            ordered.insert(key.into(), value);
        }
    }
    ordered.extend(std::mem::take(fields));
    Value::Object(ordered)
}

fn home() -> Result<String> {
    std::env::home_dir()
        .and_then(|home| home.into_os_string().into_string().ok())
        .ok_or_else(|| {
            CliError::coded("INTERNAL", "Could not determine the home directory.", false)
        })
}

enum Directory {
    Resolve,
    Complete,
    Create,
}
/// 0.9 resolved prompt paths against the home directory, never the CLI's cwd.
fn directory(path: &str, operation: Directory) -> Result<Value> {
    use shellfox_files::{Access, FileContext, FileService, Location};
    let home = home()?;
    let context = FileContext {
        cwd: home.clone(),
        home: Some(home),
    };
    let files = FileService::local_desktop(Access::ReadWrite);
    let location = Location {
        path: path.to_owned(),
    };
    let result = match operation {
        Directory::Resolve => files
            .resolve_directory_with_context(&location, Some(&context))
            .map(|r| json!({ "cwd": r.cwd })),
        Directory::Create => files
            .create_directory_with_context(&location, Some(&context))
            .map(|r| json!({ "cwd": r.cwd })),
        Directory::Complete => files
            .complete_directory_with_context(&location, Some(&context))
            .map(|r| json!({ "matches": r.matches })),
    };
    result.map_err(crate::manager_local_files::file_failure)
}
