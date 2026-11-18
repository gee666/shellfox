//! Shared saved metadata/files/lifecycle handlers, independent of delivery profile.
use super::*;
pub(in crate::remote) fn execute(
    state: &mut RemoteState,
    service: &mut Service,
    actor: &Actor,
    view_id: v2::Id,
    command: v2::Command,
) -> Result<serde_json::Value, AppError> {
    use v2::Command::*;
    match command {
        GetSnapshot(_) => {
            let snapshot = catalog::snapshot(state, service)?;
            state
                .metadata_v2
                .get_mut(&view_id)
                .ok_or_else(|| failure(ErrorCode::AuthFailed, "Metadata view missing."))?
                .revision = snapshot.revision;
            checked(snapshot)
        }
        GetTerminalProfiles(_) => {
            let mut profiles = state
                .profiles
                .clone()
                .ok_or_else(|| capability_unavailable("authoritative native terminal profiles"))?;
            profiles
                .validate()
                .map_err(|_| failure(ErrorCode::Internal, "Invalid host profiles."))?;
            profiles
                .profiles
                .retain(|profile| profile.elevation.is_none());
            if profiles
                .default_profile_id
                .as_ref()
                .is_some_and(|id| !profiles.profiles.iter().any(|profile| &profile.id == id))
            {
                profiles.default_profile_id = None;
            }
            checked(profiles)
        }
        GetHomeDirectory(_) => checked(
            state
                .home
                .clone()
                .ok_or_else(|| capability_unavailable("authoritative home directory"))?,
        ),
        CreateSession(input) => launch::create(state, service, input.0),
        AddTab(input) => launch::add(state, service, input.0),
        ActivateSession(input) => launch::activate(state, service, input.0),
        RenameSession(input) => {
            target(state, service, input.0.session_id)?;
            service
                .host()
                .rename_session(input.0.session_id, &input.0.title)
                .map_err(host_error)?;
            session_reply(state, service, input.0.session_id)
        }
        RenameTab(input) => {
            let session = target(state, service, input.0.session_id)?;
            if !session.tabs.iter().any(|tab| tab.id == input.0.tab_id) {
                return Err(failure(ErrorCode::NotFound, "Authorized tab not found."));
            }
            service
                .host()
                .rename_tab(input.0.tab_id, &input.0.title)
                .map_err(host_error)?;
            session_reply(state, service, session.id)
        }
        ReorderTabs(input) => {
            let session = target(state, service, input.0.session_id)?;
            let visible = session
                .tabs
                .iter()
                .filter(|tab| tab_visible(state, service, tab))
                .map(|tab| tab.id)
                .collect::<std::collections::HashSet<_>>();
            if input.0.tab_ids.len() != visible.len()
                || input.0.tab_ids.iter().any(|id| !visible.contains(id))
            {
                return Err(failure(
                    ErrorCode::NotFound,
                    "Authorized tab order does not match this session.",
                ));
            }
            let mut requested = input.0.tab_ids.into_iter();
            let merged = session
                .tabs
                .iter()
                .map(|tab| {
                    if visible.contains(&tab.id) {
                        requested.next().ok_or_else(|| {
                            failure(ErrorCode::Validation, "Authorized tab order is incomplete.")
                        })
                    } else {
                        Ok(tab.id)
                    }
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            service
                .host()
                .reorder_tabs(input.0.session_id, &merged)
                .map_err(host_error)?;
            session_reply(state, service, input.0.session_id)
        }
        SetSessionPinned(input) => {
            target(state, service, input.0.session_id)?;
            service
                .host()
                .set_session_pinned(input.0.session_id, input.0.pinned)
                .map_err(host_error)?;
            session_reply(state, service, input.0.session_id)
        }
        SettleSession(input) => {
            target(state, service, input.0.session_id)?;
            service
                .host()
                .set_session_settled(input.0.session_id, true)
                .map_err(host_error)?;
            session_reply(state, service, input.0.session_id)
        }
        UnsettleSession(input) => {
            target(state, service, input.0.session_id)?;
            service
                .host()
                .set_session_settled(input.0.session_id, false)
                .map_err(host_error)?;
            session_reply(state, service, input.0.session_id)
        }
        SetSessionEnv(input) => {
            target(state, service, input.0.session_id)?;
            service
                .host()
                .set_session_env(input.0.session_id, input.0.env)
                .map_err(host_error)?;
            session_reply(state, service, input.0.session_id)
        }
        CloseTab(input) => {
            let session = tab_target(state, service, input.0.tab_id)?;
            if service
                .terminals
                .get(&input.0.tab_id)
                .is_none_or(|terminal| terminal.runtime.identity().generation != input.0.generation)
            {
                return Err(failure(
                    ErrorCode::TargetLost,
                    "Original terminal generation changed.",
                ));
            }
            service.close_tab(input.0.tab_id).map_err(|_| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Original source cleanup unconfirmed; metadata retained.",
                )
            })?;
            session_reply(state, service, session.id)
        }
        DeleteSession(input) => {
            let session = target(state, service, input.0.session_id)?;
            authorize_session_sources(state, service, &session)?;
            service.delete_session(input.0.session_id).map_err(|_| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Original source cleanup unconfirmed; metadata retained.",
                )
            })?;
            checked(wire::Deleted { deleted: true })
        }
        SetTerminalEnv(input) => {
            let settings = service
                .host()
                .save_global_settings(shellfox_core::SettingsPatch {
                    terminal_env: Some(input.0.env),
                    ..Default::default()
                })
                .map_err(host_error)?;
            checked(super::settings::project(&settings))
        }
        SetVisibleSessions(input) => {
            for id in &input.0.session_ids {
                target(state, service, *id)?;
            }
            state
                .metadata_v2
                .get_mut(&view_id)
                .ok_or_else(|| failure(ErrorCode::AuthFailed, "Metadata view missing."))?
                .visible = input.0.session_ids;
            checked(wire::Configured { configured: true })
        }
        AttachTerminal(_) | AcknowledgeTerminal(_) | DetachTerminal(_) => Err(failure(
            ErrorCode::Validation,
            "Terminal operations require their typed profile dispatcher.",
        )),
        GetSessionStats(input) => statistics(state, service, input.0.session_id),
        ClearSessionError(input) => {
            target(state, service, input.0.session_id)?;
            Err(capability_unavailable(
                "actual runtime error acknowledgement; failed startup evidence is retained",
            ))
        }
        command @ (ListFiles(_)
        | ReadFile(_)
        | WriteFile(_)
        | BeginFileRead(_)
        | ReadFileChunk(_)
        | BeginFileWrite(_)
        | WriteFileChunk(_)
        | FinishFileWrite(_)
        | CancelFileTransfer(_)
        | WriteImageFile(_)
        | ReadFileAsset(_)
        | ResolveDirectory(_)
        | CreateDirectory(_)
        | CompleteDirectory(_)) => {
            let legacy =
                shellfox_remote::observer_policy::legacy_command(&command).ok_or_else(|| {
                    failure(ErrorCode::Validation, "Missing typed manager operation.")
                })?;
            authorize_file_target(state, service, &legacy)?;
            state.bind(service).execute(actor, legacy)
        }
        TakeTerminalControl(_)
        | WriteTerminal(_)
        | ResizeTerminal(_)
        | BeginTerminalUpload(_)
        | WriteTerminalUpload(_)
        | FinishTerminalUpload(_)
        | CancelTerminalUpload(_) => Err(failure(
            ErrorCode::Unsupported,
            "observer-v2 does not grant input, takeover, layout or terminal-upload authority.",
        )),
    }
}
fn session_reply(
    state: &RemoteState,
    service: &Service,
    id: SessionId,
) -> Result<serde_json::Value, AppError> {
    checked(catalog::entry(
        state,
        service,
        &target(state, service, id)?,
    )?)
}
fn authorize_file_target(
    state: &RemoteState,
    service: &Service,
    command: &wire::RemoteCommand,
) -> Result<(), AppError> {
    use wire::RemoteCommand::*;
    let (session, tab) = match command {
        ListFiles(v) => (v.session_id, v.tab_id),
        ReadFile(v) => (v.session_id, v.tab_id),
        WriteFile(v) => (v.session_id, v.tab_id),
        BeginFileRead(v) => (v.session_id, v.tab_id),
        BeginFileWrite(v) => (v.session_id, v.tab_id),
        WriteImageFile(v) => (v.session_id, v.tab_id),
        ReadFileAsset(v) => (v.session_id, v.tab_id),
        CreateDirectory(v) => (v.session_id, v.tab_id),
        _ => return Ok(()),
    };
    if let Some(id) = session {
        let session = target(state, service, id)?;
        if tab.is_some_and(|id| {
            !session
                .tabs
                .iter()
                .any(|tab| tab.id == id && tab_visible(state, service, tab))
        }) {
            return Err(failure(ErrorCode::NotFound, "Authorized tab not found."));
        }
    } else if let Some(tab) = tab {
        tab_target(state, service, tab)?;
    }
    Ok(())
}
fn statistics(
    state: &RemoteState,
    service: &Service,
    selected: Option<SessionId>,
) -> Result<serde_json::Value, AppError> {
    let snapshot = catalog::snapshot(state, service)?;
    let mut sessions = Vec::new();
    let mut tabs = Vec::new();
    if let Some(id) = selected {
        target(state, service, id)?;
    }
    for entry in snapshot.entries {
        if selected.is_some_and(|id| id.to_string() != entry.id().to_string()) {
            continue;
        }
        if let v2::CatalogEntry::Session { mut value } = entry {
            value.facts.env = catalog::known(Vec::new());
            for tab in &value.tabs {
                tabs.push(wire::TabStat { tab_id: tab.id.to_string().parse().map_err(|_| failure(ErrorCode::Internal, "Invalid tab identity."))?, processes: Vec::new(), memory_bytes: None, elapsed_seconds: None,
                reason: Some("Authoritative resource measurements are unavailable; no process was probed or inferred.".into()) });
            }
            sessions.push(*value);
        } else {
            return Err(failure(ErrorCode::Unsupported, "An authorized statistics entry cannot be represented; no partial result was returned."));
        }
    }
    checked(v2::SessionStats { sessions, tabs })
}
