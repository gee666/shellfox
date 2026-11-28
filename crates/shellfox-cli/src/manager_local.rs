//! Manager methods on the local host, built on the operations the desktop uses.
use crate::{output::CliError, session_view};
use anyhow::Result;
use serde_json::{json, Value};
use shellfox_core::{SessionId, Snapshot, TabId};
use shellfox_host::HostRequest;
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

pub(crate) fn not_found(message: &str) -> anyhow::Error {
    CliError::coded("NOT_FOUND", message, false)
}
pub(crate) fn unexpected() -> anyhow::Error {
    anyhow::anyhow!("Unexpected host response.")
}

impl<'a> Local<'a> {
    pub fn new(client: &'a LocalClient) -> Self {
        Self(client, Mutex::default())
    }
    pub async fn snapshot(&self) -> Result<Snapshot> {
        self.local_api(LocalApiRequest::Snapshot {}).await
    }
    pub(crate) async fn local_api(&self, request: LocalApiRequest) -> Result<Snapshot> {
        match self.0.execute(request).await? {
            LocalApiResponse::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(unexpected()),
        }
    }
    pub(crate) async fn desktop(&self, request: DesktopRequest) -> Result<DesktopReply> {
        self.0.request(&request).await
    }
    pub(crate) async fn view(&self, request: DesktopRequest) -> Result<Snapshot> {
        match self.desktop(request).await? {
            DesktopReply::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(unexpected()),
        }
    }
    pub(crate) async fn metadata(&self, request: HostRequest) -> Result<Snapshot> {
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
    pub(crate) async fn settings(&self, request: SettingsRequest) -> Result<Value> {
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
    pub(crate) async fn session(&self, snapshot: &Snapshot, id: SessionId) -> Result<Value> {
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| not_found("Session not found."))?;
        let mut records = self.records_of(std::slice::from_ref(session)).await?;
        records.pop().ok_or_else(|| not_found("Session not found."))
    }
    pub(crate) async fn records(&self, snapshot: &Snapshot) -> Result<Vec<Value>> {
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
    pub(crate) async fn folder_profile(
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
        if live
            .zip(generation)
            .is_some_and(|(live, given)| live != given)
        {
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
    pub(crate) async fn created(&self, request_id: SessionId) -> Result<SessionId> {
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
    pub(crate) async fn after(&self, id: SessionId, snapshot: Result<Snapshot>) -> Result<Value> {
        self.session(&snapshot?, id).await
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
