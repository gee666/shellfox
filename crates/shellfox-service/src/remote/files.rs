use super::{failure, host_error};
use crate::Service;
use shellfox_protocol::{self as wire, AppError, ErrorCode, SessionId, TabId};
use shellfox_remote::{file_ops, Actor};
use std::path::Path;

fn resolve_path(
    service: &Service,
    session: Option<SessionId>,
    tab: Option<TabId>,
    path: &mut String,
) -> Result<(), AppError> {
    let Some(id) = session else {
        return if tab.is_none() {
            Ok(())
        } else {
            Err(failure(
                ErrorCode::Validation,
                "File tab requires a session.",
            ))
        };
    };
    let snapshot = service.host().snapshot().map_err(host_error)?;
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| failure(ErrorCode::NotFound, "File session not found."))?;
    let base = match tab {
        Some(id) => {
            &session
                .tabs
                .iter()
                .find(|t| t.id == id)
                .ok_or_else(|| {
                    failure(ErrorCode::NotFound, "File tab does not belong to session.")
                })?
                .cwd
        }
        None => &session.cwd,
    };
    let requested = Path::new(path);
    if !requested.is_absolute() {
        if requested.has_root()
            || matches!(
                requested.components().next(),
                Some(std::path::Component::Prefix(_))
            )
        {
            return Err(failure(
                ErrorCode::Validation,
                "File path needs an absolute drive or a session-relative path.",
            ));
        }
        *path = Path::new(base)
            .join(requested)
            .to_str()
            .ok_or_else(|| failure(ErrorCode::Validation, "File path is not UTF-8."))?
            .to_owned();
    }
    Ok(())
}
pub fn list(
    service: &Service,
    actor: &Actor,
    mut input: wire::ListFiles,
) -> Result<serde_json::Value, AppError> {
    resolve_path(service, input.session_id, input.tab_id, &mut input.path)?;
    file_ops::list(actor, &input.path)
}
pub fn read(
    service: &Service,
    actor: &Actor,
    mut input: wire::ReadFile,
) -> Result<serde_json::Value, AppError> {
    resolve_path(service, input.session_id, input.tab_id, &mut input.path)?;
    file_ops::read(actor, &input.path)
}
pub fn write(
    service: &Service,
    actor: &Actor,
    mut input: wire::files::WriteFile,
) -> Result<serde_json::Value, AppError> {
    resolve_path(service, input.session_id, input.tab_id, &mut input.path)?;
    file_ops::write(actor, &input)
}
pub fn directory(
    service: &Service,
    actor: &Actor,
    mut input: wire::CreateDirectory,
    home: Option<&str>,
) -> Result<serde_json::Value, AppError> {
    resolve_path(service, input.session_id, input.tab_id, &mut input.path)?;
    shellfox_remote::directories::create(actor, &input, home)
}
pub fn asset(
    service: &Service,
    actor: &Actor,
    mut input: wire::ReadFileAsset,
) -> Result<serde_json::Value, AppError> {
    resolve_path(service, input.session_id, input.tab_id, &mut input.path)?;
    shellfox_remote::assets::read(actor, &input)
}
pub fn image(
    service: &Service,
    actor: &Actor,
    mut input: wire::WriteImageFile,
) -> Result<serde_json::Value, AppError> {
    resolve_path(service, input.session_id, input.tab_id, &mut input.path)?;
    shellfox_remote::assets::write(actor, &input)
}
pub fn transfer(
    service: &Service,
    actor: &Actor,
    mut command: wire::RemoteCommand,
    transfers: &mut Option<shellfox_remote::RemoteTransfers>,
) -> Result<serde_json::Value, AppError> {
    match &mut command {
        wire::RemoteCommand::BeginFileRead(input) => {
            resolve_path(service, input.session_id, input.tab_id, &mut input.path)?
        }
        wire::RemoteCommand::BeginFileWrite(input) => {
            resolve_path(service, input.session_id, input.tab_id, &mut input.path)?
        }
        _ => {}
    }
    transfers
        .as_mut()
        .ok_or_else(|| {
            shellfox_remote::capability_unavailable(
                "owner-private transfer journal not initialized",
            )
        })?
        .request(actor, &command)
}
