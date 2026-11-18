use super::*;
use shellfox_runtime::{Command, Lease, Response};
pub(super) fn current(
    state: &mut RemoteState,
    service: &mut Service,
    id: m::Id,
    reference: &m::ControlRef,
) -> Result<(Source, Lease), AppError> {
    let source = control_for(
        state
            .manager_views
            .entries
            .get_mut(&id)
            .ok_or_else(|| failure(ErrorCode::AuthFailed, "Manager view missing."))?,
        reference,
    )?;
    if service
        .manager_v2_mailboxes
        .revocation(&source.delivery)
        .is_some()
    {
        return Err(failure(
            ErrorCode::TargetLost,
            "The actual controller lease was revoked; acquire fresh references.",
        ));
    }
    if !allowed(state, service, &source) {
        revoke(state, service, id, &source, m::RevokedReason::PolicyChanged)?;
        return Err(failure(
            ErrorCode::TargetLost,
            "Native source policy or generation changed.",
        ));
    }
    let lease = service
        .manager_v2_mailboxes
        .get(source.identity, source.role)
        .ok_or_else(native_error)?
        .ready_control(reference)
        .map_err(|_| {
            failure(
                ErrorCode::TargetLost,
                "Consume current full recovery before controller side effects.",
            )
        })?;
    Ok((source, lease))
}
pub(super) fn revoke(
    state: &mut RemoteState,
    service: &mut Service,
    id: m::Id,
    source: &Source,
    reason: m::RevokedReason,
) -> Result<(), AppError> {
    service
        .manager_v2_mailboxes
        .revoke(source.identity, source.role, reason);
    let mut cleanup = Ok(());
    if let Some(view) = state.manager_views.entries.get_mut(&id) {
        if let Some(current) = view.sources.get_mut(&source.delivery.delivery_id) {
            current.revoked = true;
        }
        if let Some(inbox) = &mut state.manager_uploads {
            cleanup = inbox
                .cancel_actor(&uploads::owner(&view.connection))
                .map_err(uploads::file_error);
        }
    }
    let retirement = service
        .pump_manager_retirements()
        .map_err(|_| native_error());
    cleanup.and(retirement)
}
pub(super) fn ack(
    state: &mut RemoteState,
    service: &mut Service,
    id: m::Id,
    input: m::AcknowledgeTerminal,
) -> Result<serde_json::Value, AppError> {
    let source = source_for(
        state
            .manager_views
            .entries
            .get_mut(&id)
            .ok_or_else(native_error)?,
        &input.delivery,
    )?
    .clone();
    if !allowed(state, service, &source) {
        revoke(state, service, id, &source, m::RevokedReason::PolicyChanged)?;
        return Err(failure(ErrorCode::TargetLost, "Manager source revoked."));
    }
    let command = service
        .manager_v2_mailboxes
        .get(source.identity, source.role)
        .ok_or_else(native_error)?
        .ack_command(&input.delivery, &input.ack)
        .map_err(|_| {
            failure(
                ErrorCode::Validation,
                "ACK does not match offered bytes or exact opaque recovery.",
            )
        })?;
    let response = service
        .call(source.identity.tab_id, command)
        .map_err(|_| native_error())?;
    if !allowed(state, service, &source) {
        revoke(state, service, id, &source, m::RevokedReason::PolicyChanged)?;
        return Err(failure(
            ErrorCode::TargetLost,
            "Manager source changed during ACK.",
        ));
    }
    service
        .manager_v2_mailboxes
        .get(source.identity, source.role)
        .ok_or_else(native_error)?
        .commit_ack(&input.delivery, &input.ack, &response)
        .map_err(|_| {
            failure(
                ErrorCode::TargetLost,
                "Recovery/layout changed before ACK commit.",
            )
        })?;
    checked(wire::Acknowledged { acknowledged: true })
}
pub(super) fn detach(
    state: &mut RemoteState,
    service: &mut Service,
    id: m::Id,
    input: m::DetachTerminal,
) -> Result<serde_json::Value, AppError> {
    let source = source_for(
        state
            .manager_views
            .entries
            .get_mut(&id)
            .ok_or_else(native_error)?,
        &input.delivery,
    )?
    .clone();
    match source.role {
        Role::Controller(_) => {
            if !source.revoked
                && service
                    .manager_v2_mailboxes
                    .revocation(&source.delivery)
                    .is_none()
                && input.release_control.as_ref() != source.control.as_ref()
            {
                return Err(failure(
                    ErrorCode::AuthFailed,
                    "Detaching controller delivery requires its exact ControlRef release.",
                ));
            }
        }
        Role::Observer(_) => {
            if input.release_control.is_some() {
                return Err(failure(
                    ErrorCode::AuthFailed,
                    "Observation has no controller to release.",
                ));
            }
        }
    }
    revoke(state, service, id, &source, m::RevokedReason::ViewDetached)?;
    state
        .manager_views
        .entries
        .get_mut(&id)
        .ok_or_else(native_error)?
        .sources
        .remove(&input.delivery.delivery_id);
    service.manager_v2_mailboxes.forget_notice(&source.delivery);
    checked(wire::Detached { detached: true })
}
pub(super) fn resize(
    state: &mut RemoteState,
    service: &mut Service,
    id: m::Id,
    input: m::ResizeTerminal,
) -> Result<serde_json::Value, AppError> {
    let (source, lease) = current(state, service, id, &input.control)?;
    let book = service
        .manager_v2_mailboxes
        .get(source.identity, source.role)
        .ok_or_else(native_error)?;
    if source.layout.as_ref() != Some(&input.layout)
        || book.layout_revision() != Some(input.expected_layout_revision)
    {
        return Err(failure(
            ErrorCode::TargetLost,
            "Layout authority or expected canonical geometry is stale.",
        ));
    }
    let response = service
        .call(
            source.identity.tab_id,
            Command::Resize {
                lease,
                cols: input.cols,
                rows: input.rows,
            },
        )
        .map_err(|_| native_error())?;
    if !matches!(response, Response::Applied) {
        return Err(native_error());
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    loop {
        service
            .drain(source.identity.tab_id)
            .map_err(|_| native_error())?;
        if !allowed(state, service, &source) {
            revoke(state, service, id, &source, m::RevokedReason::PolicyChanged)?;
            return Err(failure(
                ErrorCode::TargetLost,
                "Source policy changed while resizing.",
            ));
        }
        let book = service
            .manager_v2_mailboxes
            .get(source.identity, source.role)
            .ok_or_else(native_error)?;
        if let Some(layout) = book.current_layout() {
            if layout.cols == input.cols
                && layout.rows == input.rows
                && layout.layout_revision >= input.expected_layout_revision
            {
                return checked(m::Resized {
                    cols: layout.cols,
                    rows: layout.rows,
                    layout_revision: layout.layout_revision,
                });
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(failure(
                ErrorCode::NativeUnavailable,
                "Resize outcome is uncertain; no synthetic geometry revision was returned.",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
