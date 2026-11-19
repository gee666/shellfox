use super::{Actor, AppError, ErrorCode, RemoteService, Service, checked, failure, host_error, key};
use crate::wsl_files_worker::Worker;
use shellfox_core::{SessionId, TabId, WslLaunchAccount};
use shellfox_files::{self as files, WslBrowserScope, wsl_files_ipc::{Reply, Request}};
use shellfox_protocol::{self as wire, RemoteCommand, Validate};
use shellfox_remote::{ViewPermission, assets, file_ops};

#[path = "remote_wsl_transfers.rs"]
mod transfer;
pub(in crate::remote) use transfer::Transfers;

#[derive(Clone)]
struct Scope {
    session: SessionId,
    tab: TabId,
    guest: WslBrowserScope,
    account: WslLaunchAccount,
}

impl Scope {
    fn resolve(service: &Service, session: Option<SessionId>, tab: Option<TabId>) -> Result<Option<Self>, AppError> {
        let (Some(session), Some(tab)) = (session, tab) else { return Ok(None); };
        let snapshot = service.host().snapshot().map_err(host_error)?;
        let tab = snapshot.sessions.iter().find(|value| value.id == session)
            .and_then(|session| session.tabs.iter().find(|value| value.id == tab))
            .ok_or_else(|| failure(ErrorCode::NotFound, "The file tab is not in this session."))?;
        let profile = tab.profile_id.as_str();
        if profile != "wsl" && !profile.starts_with("wsl:") { return Ok(None); }
        let account = tab.wsl_account().cloned().ok_or_else(|| {
            failure(ErrorCode::AccessDenied, "Open a new WSL tab with a verified launch user before using Files.")
        })?;
        let guest = WslBrowserScope::for_tab(&tab.cwd, profile).map_err(file_ops::file_error)?;
        Ok(Some(Self { session, tab: tab.id, guest, account }))
    }

    fn location(&self, path: &str) -> Result<files::Location, AppError> {
        Ok(files::Location { path: self.guest.guest(path).map_err(file_ops::file_error)? })
    }

    fn start(&self) -> Result<Worker, AppError> {
        Worker::start(self.guest.distro(), &self.account).map_err(worker_error)
    }
}

impl RemoteService<'_> {
    pub(super) fn route_wsl_file(&mut self, actor: &Actor, command: &RemoteCommand)
        -> Result<Option<serde_json::Value>, AppError> {
        use RemoteCommand::*;
        let (session, tab, path, write) = match command {
            ListFiles(v) => (v.session_id, v.tab_id, &v.path, false),
            ReadFile(v) => (v.session_id, v.tab_id, &v.path, false),
            ReadFileAsset(v) => (v.session_id, v.tab_id, &v.path, false),
            WriteFile(v) => (v.session_id, v.tab_id, &v.path, true),
            WriteImageFile(v) => (v.session_id, v.tab_id, &v.path, true),
            CreateDirectory(v) => (v.session_id, v.tab_id, &v.path, true),
            BeginFileRead(v) => (v.session_id, v.tab_id, &v.path, false),
            BeginFileWrite(v) => (v.session_id, v.tab_id, &v.path, true),
            ResolveDirectory(v) => (None, None, &v.path, false),
            CompleteDirectory(v) => (None, None, &v.path, false),
            ReadFileChunk(_) | WriteFileChunk(_) | FinishFileWrite(_) | CancelFileTransfer(_) => {
                return self.state.wsl_transfers.continue_request(self.service, actor, command);
            }
            _ => return Ok(None),
        };
        let Some(scope) = Scope::resolve(self.service, session, tab)? else {
            if requires_wsl_scope(path) {
                return Err(failure(ErrorCode::AccessDenied, "Choose the WSL tab before opening its guest files."));
            }
            return Ok(None);
        };
        authorize(actor, path, write)?;
        command.validate().map_err(|_| failure(ErrorCode::Validation, "Invalid WSL Files request."))?;
        let location = scope.location(path)?;
        if matches!(command, BeginFileRead(_) | BeginFileWrite(_)) {
            return self.state.wsl_transfers.begin(actor, scope, location, command).map(Some);
        }
        inline(scope, location, command).map(Some)
    }
}

fn authorize(actor: &Actor, path: &str, write: bool) -> Result<(), AppError> {
    file_ops::require_full_account(actor, path)?;
    if write && actor.permission != ViewPermission::Write {
        return Err(failure(ErrorCode::AccessDenied, "This approval does not allow file writes."));
    }
    Ok(())
}

fn requires_wsl_scope(path: &str) -> bool {
    let value = path.trim().trim_matches(['"', '\'']);
    if value.starts_with('/') { return true; }
    let Some(unc) = value.strip_prefix(r"\\") else { return false; };
    let mut parts = unc.split(['\\', '/']);
    let mut server = parts.next().unwrap_or_default();
    if server == "?" && parts.next().is_some_and(|part| part.eq_ignore_ascii_case("UNC")) {
        server = parts.next().unwrap_or_default();
    }
    server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost")
}

fn inline(scope: Scope, location: files::Location, command: &RemoteCommand) -> Result<serde_json::Value, AppError> {
    use RemoteCommand::*;
    let mut payload = Vec::new();
    let request = match command {
        ListFiles(_) => Request::List(location),
        ReadFile(_) => Request::ReadInline(location),
        CreateDirectory(_) => Request::CreateDirectory(location),
        WriteFile(input) => {
            payload = input.text.as_bytes().to_vec();
            Request::WriteInline(files::WriteText { path: location.path, text: String::new(), revision: input.revision.clone() })
        }
        ReadFileAsset(input) => Request::AssetInline(files::AssetRequest {
            path: location.path, asset_path: input.asset_path.clone(),
        }),
        WriteImageFile(input) => {
            let mut image = assets::decode_image_write(input)?;
            image.path = location.path;
            payload = std::mem::take(&mut image.bytes);
            Request::WriteImageInline(image)
        }
        _ => return Err(invalid_reply()),
    };
    let (reply, bytes) = scope.start()?.exchange(request, payload).map_err(worker_error)?;
    match (command, reply) {
        (_, Reply::Error(error)) if bytes.is_empty() => Err(file_ops::file_error(error)),
        (ListFiles(_), Reply::List(list)) if bytes.is_empty() => file_ops::list_value(list),
        (CreateDirectory(_), Reply::List(list)) if bytes.is_empty() => checked(wire::Directory { cwd: list.path }),
        (ReadFile(_), Reply::Document(document)) => file_ops::read_value(document_payload(document, bytes)?),
        (ReadFileAsset(_), Reply::Asset(mut asset)) if asset.bytes.is_empty() && asset.size == bytes.len() as u64 => {
            asset.bytes = bytes;
            assets::asset_value(asset)
        }
        (WriteFile(_) | WriteImageFile(_), Reply::Saved(value)) if bytes.is_empty() => checked(wire::files::FileWriteResult {
            path: value.path, size: value.size, revision: value.revision,
        }),
        _ => Err(invalid_reply()),
    }
}

fn document_payload(mut document: files::ReadResult, bytes: Vec<u8>) -> Result<files::ReadResult, AppError> {
    match &mut document {
        files::ReadResult::Text { size, text, .. } if text.is_empty() && *size == bytes.len() as u64 => {
            *text = String::from_utf8(bytes).map_err(|_| invalid_reply())?;
            if text.contains('\0') { return Err(invalid_reply()); }
        }
        files::ReadResult::Image { size, bytes: data, .. } if data.is_empty() && *size == bytes.len() as u64 => *data = bytes,
        files::ReadResult::Binary { .. } if bytes.is_empty() => {},
        _ => return Err(invalid_reply()),
    }
    Ok(document)
}

fn worker_error(error: anyhow::Error) -> AppError {
    match error.downcast::<files::FileError>() {
        Ok(error) => file_ops::file_error(error),
        Err(_) => failure(ErrorCode::Unsupported, "WSL Files could not use the tab's verified guest account. No operation was retried."),
    }
}

fn invalid_reply() -> AppError {
    failure(ErrorCode::Internal, "Invalid WSL Files worker response.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unscoped_guest_paths_cannot_fall_through_to_windows() {
        for path in ["/tmp", r"\\wsl$\Debian\tmp", r"\\wsl.LOCALHOST\Debian", r"\\?\UNC\wsl$\Debian", r#""\\wsl$\Debian""#] {
            assert!(requires_wsl_scope(path), "{path}");
        }
        for path in [r"C:\tmp", "C:/tmp", "~", ".", r"\\server\share"] {
            assert!(!requires_wsl_scope(path), "{path}");
        }
    }
}
