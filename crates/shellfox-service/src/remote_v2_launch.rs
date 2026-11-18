use super::*;
pub(in crate::remote) fn ensure_launch_profile(state: &RemoteState, id: &str) -> Result<(), AppError> {
    let profiles = state
        .profiles
        .as_ref()
        .ok_or_else(|| capability_unavailable("trusted manager launch profiles"))?;
    profiles
        .validate()
        .map_err(|_| failure(ErrorCode::Internal, "Invalid native profile evidence."))?;
    let profile = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| failure(ErrorCode::NotFound, "Authorized launch profile not found."))?;
    if profile.elevation.is_some() {
        return Err(failure(
            ErrorCode::AccessDenied,
            "Elevated launch remains local-only.",
        ));
    }
    if !profile.available {
        return Err(capability_unavailable("requested native shell launch"));
    }
    Ok(())
}
pub(super) fn create(
    state: &mut RemoteState,
    service: &mut Service,
    input: wire::CreateSession,
) -> Result<serde_json::Value, AppError> {
    let profile = service
        .host()
        .snapshot()
        .map_err(host_error)?
        .preferences
        .default_profile_id;
    let previous = service
        .host()
        .creation_request_receipt(input.request_id)
        .map_err(host_error)?;
    if let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } = previous {
        return checked(catalog::entry(
            state,
            service,
            &target(state, service, session_id)?,
        )?);
    }
    ensure_launch_profile(state, profile.as_str())?;
    let snapshot = service
        .create_application_session(shellfox_core::CreateSession {
            request_id: input.request_id,
            title: input.title,
            cwd: input.cwd,
            profile_id: profile,
        })
        .map_err(|_| {
            failure(
                ErrorCode::NativeUnavailable,
                "Trusted session creation failed; no operation was retried.",
            )
        })?;
    let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } = service
        .host()
        .creation_request_receipt(input.request_id)
        .map_err(host_error)?
    else {
        return Err(failure(
            ErrorCode::TargetLost,
            "Created session receipt missing.",
        ));
    };
    let session = snapshot
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| {
            failure(
                ErrorCode::TargetLost,
                "Original created session is unavailable.",
            )
        })?;
    if matches!(
        previous,
        shellfox_host::CreationRequestReceipt::ReceiptUnavailable
    ) {
        for tab in &session.tabs {
            if !service.terminals.contains_key(&tab.id) {
                service.start_terminal(tab.id).map_err(|_| {
                    failure(
                        ErrorCode::NativeUnavailable,
                        "Session launch is uncertain; original owner retained.",
                    )
                })?;
            }
        }
    }
    checked(catalog::entry(state, service, session)?)
}
pub(super) fn add(
    state: &mut RemoteState,
    service: &mut Service,
    input: wire::AddTab,
) -> Result<serde_json::Value, AppError> {
    let session = target(state, service, input.session_id)?;
    let profile = match input.profile_id {
        Some(value) => shellfox_core::ProfileId::new(value).map_err(host_error)?,
        None => {
            service
                .host()
                .snapshot()
                .map_err(host_error)?
                .preferences
                .default_profile_id
        }
    };
    ensure_launch_profile(state, profile.as_str())?;
    let snapshot = service
        .host()
        .add_tab(
            session.id,
            shellfox_core::AddTab {
                title: input.title,
                cwd: input.cwd,
                profile_id: profile,
            },
        )
        .map_err(host_error)?;
    let tab = snapshot
        .sessions
        .iter()
        .find(|value| value.id == session.id)
        .and_then(|value| {
            value
                .tabs
                .iter()
                .find(|tab| !session.tabs.iter().any(|old| old.id == tab.id))
        })
        .map(|tab| tab.id)
        .ok_or_else(|| {
            failure(
                ErrorCode::TargetLost,
                "Created tab identity is unavailable.",
            )
        })?;
    service.start_terminal(tab).map_err(|_| {
        failure(
            ErrorCode::NativeUnavailable,
            "Tab launch is uncertain; original owner retained.",
        )
    })?;
    checked(catalog::entry(
        state,
        service,
        &target(state, service, session.id)?,
    )?)
}
pub(super) fn activate(
    state: &mut RemoteState,
    service: &mut Service,
    input: wire::ActivateSession,
) -> Result<serde_json::Value, AppError> {
    let session = target(state, service, input.session_id)?;
    authorize_session_sources(state, service, &session)?;
    if let Some(tab) = session
        .tabs
        .iter()
        .find(|tab| tab.state != shellfox_core::TabState::Archived)
    {
        if !service.terminals.contains_key(&tab.id) {
            ensure_launch_profile(state, tab.profile_id.as_str())?;
            service.start_terminal(tab.id).map_err(|_| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Activation launch is uncertain; original owner retained.",
                )
            })?;
        }
    }
    service
        .host()
        .select_session(session.id)
        .map_err(host_error)?;
    checked(catalog::entry(
        state,
        service,
        &target(state, service, session.id)?,
    )?)
}
