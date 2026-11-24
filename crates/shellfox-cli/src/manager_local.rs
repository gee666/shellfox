//! Manager methods on the local host, built on the operations the desktop uses.
//! Directory helpers run here: the CLI is the same user on the same machine.
use crate::{manager::Call, output::CliError, session_view};
use anyhow::{Context, Result};
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
            LocalApiResponse::Settings(settings) => Ok(serde_json::to_value(settings)?),
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
        Ok(session_view::record(session, &self.activity().await?))
    }
    async fn records(&self, snapshot: &Snapshot) -> Result<Vec<Value>> {
        let activity = self.activity().await?;
        Ok(snapshot
            .sessions
            .iter()
            .map(|s| session_view::record(s, &activity))
            .collect())
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
    /// The host's effective default profile; see shellfox_service::effective_default_profile.
    async fn default_profile(&self, snapshot: &Snapshot) -> Result<shellfox_core::ProfileId> {
        let saved = snapshot.preferences.default_profile_id.clone();
        let DesktopReply::TerminalProfiles(catalog) =
            self.desktop(DesktopRequest::TerminalProfiles).await?
        else {
            return Err(unexpected());
        };
        let usable = shellfox_service::usable_profiles(&catalog.profiles.profiles);
        shellfox_service::effective_default_profile(saved.as_str(), usable)
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| {
                CliError::coded(
                    "UNSUPPORTED",
                    "No terminal profile is available on this host.",
                    false,
                )
            })
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
            R::CreateDirectory(input) if input.session_id.is_none() => {
                directory(&input.path, Directory::Create)
            }
            R::CreateSession(input) => {
                let before = self.snapshot().await?;
                let profile_id = match shellfox_service::desktop_launch::folder_profile(&input.cwd)
                {
                    Some(profile) => profile,
                    None => self.default_profile(&before).await?,
                };
                let request = shellfox_core::CreateSession {
                    request_id: input.request_id,
                    title: input.title,
                    cwd: input.cwd,
                    profile_id,
                };
                let after = self
                    .local_api(LocalApiRequest::CreateSession { input: request })
                    .await?;
                let created = after
                    .sessions
                    .iter()
                    .find(|s| !before.sessions.iter().any(|old| old.id == s.id))
                    .map(|s| s.id)
                    .context("The host did not return the new session.")?;
                self.session(&after, created).await
            }
            R::AddTab(input) => {
                let before = self.snapshot().await?;
                let profile_id = match input.profile_id {
                    Some(profile) => profile
                        .parse()
                        .map_err(|_| CliError::usage("Invalid profile ID."))?,
                    None => self.default_profile(&before).await?,
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
                    .metadata(HostRequest::ActivateSession {
                        session_id: input.session_id,
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
            R::CloseTab(input) => {
                if self.live(input.tab_id).await? != Some(input.generation) {
                    return Err(CliError::coded(
                        "TARGET_LOST",
                        "Stale terminal generation.",
                        false,
                    ));
                }
                let before = self.snapshot().await?;
                let session = before
                    .sessions
                    .iter()
                    .find(|s| s.tabs.iter().any(|t| t.id == input.tab_id))
                    .map(|s| s.id)
                    .ok_or_else(|| not_found("Tab not found."))?;
                let after = self
                    .local_api(LocalApiRequest::CloseTab { id: input.tab_id })
                    .await;
                self.after(session, after).await
            }
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
            command => Err(CliError::coded(
                "UNSUPPORTED",
                format!(
                    "{} is not available from the local CLI. Use the matching shellfox command.",
                    command.method_name()
                ),
                false,
            )),
        }
    }
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
    result.map_err(|error| {
        let code = match error.code {
            shellfox_files::ErrorCode::NotFound => "NOT_FOUND",
            shellfox_files::ErrorCode::Unsupported => "UNSUPPORTED",
            shellfox_files::ErrorCode::AccessDenied => "ACCESS_DENIED",
            shellfox_files::ErrorCode::StorageFailed => "STORAGE_FAILED",
            shellfox_files::ErrorCode::FileConflict => "FILE_CONFLICT",
            shellfox_files::ErrorCode::Validation => "VALIDATION",
        };
        CliError::coded(code, error.message, false)
    })
}
