//! The 0.9 manager methods a CLI call dispatches to on the local host.
use crate::{
    manager::Call,
    manager_local::{not_found, unexpected, Local},
    manager_local_files::{directory, home, Directory},
    output::CliError,
};
use anyhow::Result;
use serde_json::{json, Value};
use shellfox_host::HostRequest;
use shellfox_protocol::RemoteCommand as R;
use shellfox_service::{
    local::{DesktopReply, DesktopRequest},
    local_api::{LocalApiRequest, SettingsRequest},
};

impl Local<'_> {
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
