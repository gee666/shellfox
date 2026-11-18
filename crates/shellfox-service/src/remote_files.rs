use super::{failure, host_error};
use crate::Service;
use shellfox_protocol::{self as wire, AppError, ErrorCode, SessionId, TabId};
use shellfox_remote::{file_ops, Actor};

fn membership(
    service: &Service,
    session: Option<SessionId>,
    tab: Option<TabId>,
    path: &str,
) -> Result<(), AppError> {
    if session.is_some() && !std::path::Path::new(path).is_absolute() {
        return Err(failure(
            ErrorCode::Unsupported,
            "Relative session file paths need authoritative runtime cwd evidence.",
        ));
    }
    if let Some(id) = session {
        let snapshot = service.host().snapshot().map_err(host_error)?;
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| failure(ErrorCode::NotFound, "File session not found."))?;
        if tab.is_some_and(|id| !session.tabs.iter().any(|t| t.id == id)) {
            return Err(failure(
                ErrorCode::NotFound,
                "File tab does not belong to session.",
            ));
        }
    } else if tab.is_some() {
        return Err(failure(
            ErrorCode::Validation,
            "File tab requires a session.",
        ));
    }
    Ok(())
}
pub fn list(
    service: &Service,
    actor: &Actor,
    input: wire::ListFiles,
) -> Result<serde_json::Value, AppError> {
    membership(service, input.session_id, input.tab_id, &input.path)?;
    file_ops::list(actor, &input.path)
}
pub fn read(
    service: &Service,
    actor: &Actor,
    input: wire::ReadFile,
) -> Result<serde_json::Value, AppError> {
    membership(service, input.session_id, input.tab_id, &input.path)?;
    file_ops::read(actor, &input.path)
}
pub fn write(
    service: &Service,
    actor: &Actor,
    input: wire::files::WriteFile,
) -> Result<serde_json::Value, AppError> {
    membership(service, input.session_id, input.tab_id, &input.path)?;
    file_ops::write(actor, &input)
}
pub fn directory(
    service: &Service,
    actor: &Actor,
    input: wire::CreateDirectory,
    home: Option<&str>,
) -> Result<serde_json::Value, AppError> {
    membership(service, input.session_id, input.tab_id, &input.path)?;
    shellfox_remote::directories::create(actor, &input, home)
}
pub fn asset(
    service: &Service,
    actor: &Actor,
    input: wire::ReadFileAsset,
) -> Result<serde_json::Value, AppError> {
    membership(service, input.session_id, input.tab_id, &input.path)?;
    shellfox_remote::assets::read(actor, &input)
}
pub fn image(
    service: &Service,
    actor: &Actor,
    input: wire::WriteImageFile,
) -> Result<serde_json::Value, AppError> {
    membership(service, input.session_id, input.tab_id, &input.path)?;
    shellfox_remote::assets::write(actor, &input)
}
pub fn transfer(
    service: &Service,
    actor: &Actor,
    command: &wire::RemoteCommand,
    transfers: &mut Option<shellfox_remote::RemoteTransfers>,
) -> Result<serde_json::Value, AppError> {
    match command {
        wire::RemoteCommand::BeginFileRead(input) => {
            membership(service, input.session_id, input.tab_id, &input.path)?
        }
        wire::RemoteCommand::BeginFileWrite(input) => {
            membership(service, input.session_id, input.tab_id, &input.path)?
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
        .request(actor, command)
}
