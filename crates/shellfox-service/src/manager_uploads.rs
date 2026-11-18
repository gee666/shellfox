use super::*;
use shellfox_remote::terminal_upload as files;
pub(super) fn owner_id(id: m::Id) -> String {
    format!("manager-v2/{id}")
}
pub(super) fn owner(connection: &ManagerConnection) -> String {
    owner_id(connection.binding().connection_id)
}
fn actor(
    state: &RemoteState,
    id: m::Id,
    reference: &m::ControlRef,
) -> Result<files::UploadActor, AppError> {
    let view = state
        .manager_views
        .entries
        .get(&id)
        .ok_or_else(native_error)?;
    if !view
        .sources
        .values()
        .any(|source| source.control.as_ref() == Some(reference))
    {
        return Err(failure(
            ErrorCode::AuthFailed,
            "Upload control is not in this connection.",
        ));
    }
    let connection = files::TransferActor::new(
        owner(&view.connection),
        id.to_string().parse().map_err(|_| native_error())?,
    )
    .map_err(file_error)?;
    files::UploadActor::new(
        connection,
        files::TerminalUploadIdentity {
            tab_id: reference
                .tab_id
                .to_string()
                .parse()
                .map_err(|_| native_error())?,
            generation: reference
                .generation
                .to_string()
                .parse()
                .map_err(|_| native_error())?,
            control_epoch: reference.control_epoch,
        },
        files::UploadEnvironment::Local,
    )
    .map_err(file_error)
}
pub(super) fn request(
    state: &mut RemoteState,
    service: &mut Service,
    id: m::Id,
    command: m::Command,
) -> Result<serde_json::Value, AppError> {
    let reference = match &command {
        m::Command::BeginTerminalUpload(value) => &value.control,
        m::Command::WriteTerminalUpload(value) => &value.control,
        m::Command::FinishTerminalUpload(value) => &value.control,
        m::Command::CancelTerminalUpload(value) => &value.control,
        _ => {
            return Err(failure(
                ErrorCode::Validation,
                "Not a terminal upload operation.",
            ))
        }
    }
    .clone();
    let actor = actor(state, id, &reference)?;
    // Exact old actor/control may cancel its own staging after control loss.
    if let m::Command::CancelTerminalUpload(value) = command {
        state
            .manager_uploads
            .as_mut()
            .ok_or_else(|| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Private upload inbox is not configured.",
                )
            })?
            .cancel(
                &actor,
                &files::UploadIdentity {
                    upload_id: value
                        .upload_id
                        .to_string()
                        .parse()
                        .map_err(|_| native_error())?,
                },
            )
            .map_err(file_error)?;
        return checked(wire::Cancelled { cancelled: true });
    }
    let (source, lease) = control::current(state, service, id, &reference)?;
    let mut inbox = state.manager_uploads.take().ok_or_else(|| {
        failure(
            ErrorCode::NativeUnavailable,
            "Private upload inbox is not configured.",
        )
    })?;
    // Existing UploadInbox invokes this before staging and again at filesystem commit.
    // Owner dispatch holds the current approval/connection commit fence throughout.
    let owner = std::cell::RefCell::new(service);
    let guard = |actual: &files::UploadActor| -> files::Result<()> {
        if actual != &actor {
            return Err(files::FileError {
                code: files::ErrorCode::AccessDenied,
                message: "Upload actor/control changed.".into(),
            });
        }
        let mut service = owner.borrow_mut();
        if !allowed(state, &service, &source) {
            return Err(files::FileError {
                code: files::ErrorCode::AccessDenied,
                message: "Upload native source policy changed.".into(),
            });
        }
        let response = service
            .call(
                source.identity.tab_id,
                shellfox_runtime::Command::CheckControllerReady { lease },
            )
            .map_err(|_| files::FileError {
                code: files::ErrorCode::AccessDenied,
                message: "Upload controller is no longer ready.".into(),
            })?;
        if !matches!(response, shellfox_runtime::Response::Applied) {
            return Err(files::FileError {
                code: files::ErrorCode::AccessDenied,
                message: "Upload controller readiness failed.".into(),
            });
        }
        Ok(())
    };
    let result = (|| match command {
        m::Command::BeginTerminalUpload(value) => {
            let result = inbox
                .begin(
                    &actor,
                    &files::BeginUpload {
                        file_name: value.file_name,
                        size: value.size,
                    },
                    &guard,
                )
                .map_err(file_error)?;
            checked(m::TerminalUploadStarted {
                upload_id: result
                    .upload_id
                    .to_string()
                    .parse()
                    .map_err(|_| native_error())?,
                chunk_bytes: result.chunk_bytes,
            })
        }
        m::Command::WriteTerminalUpload(value) => {
            let bytes = if value.data.is_empty() {
                Vec::new()
            } else {
                shellfox_protocol::observer_v2::decode_bytes(&value.data, 192 * 1024).map_err(
                    |_| failure(ErrorCode::Validation, "Invalid canonical upload chunk."),
                )?
            };
            let result = inbox
                .write_chunk(
                    &actor,
                    &files::UploadChunk {
                        upload_id: value
                            .upload_id
                            .to_string()
                            .parse()
                            .map_err(|_| native_error())?,
                        offset: value.offset,
                        data: bytes,
                    },
                    &guard,
                )
                .map_err(file_error)?;
            checked(wire::TransferOffset {
                offset: result.offset,
            })
        }
        m::Command::FinishTerminalUpload(value) => {
            let result = inbox
                .finish(
                    &actor,
                    &files::UploadIdentity {
                        upload_id: value
                            .upload_id
                            .to_string()
                            .parse()
                            .map_err(|_| native_error())?,
                    },
                    &guard,
                    &files::LocalUploadPath,
                )
                .map_err(file_error)?;
            checked(wire::TerminalUploadResult {
                path: result.path,
                file_name: result.file_name,
                size: result.size,
            })
        }
        _ => Err(failure(
            ErrorCode::Validation,
            "Typed terminal upload dispatcher mismatch.",
        )),
    })();
    state.manager_uploads = Some(inbox);
    result
}
pub(super) fn file_error(error: files::FileError) -> AppError {
    shellfox_remote::file_ops::file_error(error)
}
