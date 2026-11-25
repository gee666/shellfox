//! Terminal uploads, as in 0.9: a peer that controls a terminal sends a file
//! into the host's private inbox (`<data>/uploads`) and gets back the path to
//! type into that terminal. Each step needs the same live control lease.
use super::*;
use shellfox_files as files;
use shellfox_remote::terminal_upload::{
    BeginUpload, LocalUploadPath, TerminalUploadIdentity, TransferActor, UploadActor, UploadChunk,
    UploadEnvironment, UploadIdentity, UploadInbox, UploadLimits,
};
use uuid::Uuid;

/// The terminal an upload belongs to, from any of the four requests.
fn target(
    command: &RemoteCommand,
) -> Option<(shellfox_core::TabId, shellfox_core::Generation, u64)> {
    match command {
        RemoteCommand::BeginTerminalUpload(v) => Some((v.tab_id, v.generation, v.control_epoch)),
        RemoteCommand::WriteTerminalUpload(v) => Some((v.tab_id, v.generation, v.control_epoch)),
        RemoteCommand::FinishTerminalUpload(v) => Some((v.tab_id, v.generation, v.control_epoch)),
        RemoteCommand::CancelTerminalUpload(v) => Some((v.tab_id, v.generation, v.control_epoch)),
        _ => None,
    }
}
fn file_error(code: files::ErrorCode, message: &str) -> files::FileError {
    files::FileError {
        code,
        message: message.into(),
    }
}
fn storage_failed() -> AppError {
    failure(
        ErrorCode::StorageFailed,
        "Host upload storage is unavailable or full.",
    )
}
fn uuid(id: impl std::fmt::Display) -> Result<Uuid, AppError> {
    Uuid::parse_str(&id.to_string())
        .map_err(|_| failure(ErrorCode::Validation, "Invalid terminal identity."))
}

impl RemoteService<'_> {
    pub(super) fn terminal_upload(
        &mut self,
        actor: &Actor,
        command: RemoteCommand,
    ) -> Result<serde_json::Value, AppError> {
        let (tab, generation, epoch) = target(&command)
            .ok_or_else(|| failure(ErrorCode::Internal, "Not a terminal upload operation."))?;
        let mut inbox = match self.state.uploads.take() {
            Some(inbox) => inbox,
            None => UploadInbox::open(
                self.service.directory.join("uploads"),
                UploadLimits::default(),
            )
            .map_err(|_| storage_failed())?,
        };
        let result = self.upload_step(&mut inbox, actor, command, (tab, generation, epoch));
        self.state.uploads = Some(inbox);
        result
    }

    fn upload_step(
        &self,
        inbox: &mut UploadInbox,
        actor: &Actor,
        command: RemoteCommand,
        (tab, generation, epoch): (shellfox_core::TabId, shellfox_core::Generation, u64),
    ) -> Result<serde_json::Value, AppError> {
        let connection =
            TransferActor::new(key(actor), Uuid::from_u128(u128::from(actor.view_epoch())))
                .map_err(|_| failure(ErrorCode::Validation, "Invalid upload connection."))?;
        let identity = TerminalUploadIdentity {
            tab_id: uuid(tab)?,
            generation: uuid(generation)?,
            control_epoch: epoch,
        };
        let upload = UploadActor::new(connection, identity, self.upload_environment(tab)?)
            .map_err(|_| failure(ErrorCode::Validation, "Invalid upload terminal identity."))?;
        // Every step checks again that this view still controls the terminal.
        let denied = std::cell::RefCell::new(None);
        let auth = |_: &UploadActor| -> files::Result<()> {
            self.terminal_lease(actor, tab, generation, epoch)
                .map(|_| ())
                .map_err(|error| {
                    *denied.borrow_mut() = Some(error);
                    file_error(
                        files::ErrorCode::AccessDenied,
                        "Upload terminal control changed.",
                    )
                })
        };
        // A lost lease reports as such. Storage errors never read as ACCESS_DENIED,
        // which a 0.9 client takes for a revoked device.
        let error = |error: files::FileError| -> AppError {
            if let Some(denied) = denied.borrow_mut().take() {
                return denied;
            }
            match error.code {
                files::ErrorCode::Validation => failure(ErrorCode::Validation, &error.message),
                files::ErrorCode::NotFound => {
                    failure(ErrorCode::NotFound, "Upload not found for this connection.")
                }
                _ => storage_failed(),
            }
        };
        match command {
            RemoteCommand::BeginTerminalUpload(input) => {
                let started = inbox
                    .begin(
                        &upload,
                        &BeginUpload {
                            file_name: input.file_name,
                            size: input.size,
                        },
                        &auth,
                    )
                    .map_err(error)?;
                checked(wire::TerminalUploadStarted {
                    upload_id: started.upload_id,
                    chunk_bytes: started.chunk_bytes,
                })
            }
            RemoteCommand::WriteTerminalUpload(input) => {
                let data = wire::decode_chunk(&input.data).map_err(|_| {
                    failure(ErrorCode::Validation, "Invalid upload chunk encoding.")
                })?;
                let chunk = UploadChunk {
                    upload_id: input.upload_id,
                    offset: input.offset,
                    data,
                };
                let progress = inbox.write_chunk(&upload, &chunk, &auth).map_err(error)?;
                checked(wire::TransferOffset {
                    offset: progress.offset,
                })
            }
            RemoteCommand::FinishTerminalUpload(input) => {
                let id = UploadIdentity {
                    upload_id: input.upload_id,
                };
                let finished = match &upload.environment {
                    UploadEnvironment::Local => inbox.finish(&upload, &id, &auth, &LocalUploadPath),
                    UploadEnvironment::Wsl { .. } => {
                        inbox.finish(&upload, &id, &auth, &WslUploadPath)
                    }
                }
                .map_err(error)?;
                checked(wire::TerminalUploadResult {
                    path: finished.path,
                    file_name: finished.file_name,
                    size: finished.size,
                })
            }
            RemoteCommand::CancelTerminalUpload(input) => {
                // The sender may discard its own partial after it lost control.
                inbox
                    .cancel(
                        &upload,
                        &UploadIdentity {
                            upload_id: input.upload_id,
                        },
                    )
                    .map_err(error)?;
                checked(wire::Cancelled { cancelled: true })
            }
            _ => Err(failure(
                ErrorCode::Internal,
                "Not a terminal upload operation.",
            )),
        }
    }

    /// Uploads go to the account the terminal runs as: the host, or its WSL distro.
    fn upload_environment(&self, tab: shellfox_core::TabId) -> Result<UploadEnvironment, AppError> {
        let snapshot = self.service.host().snapshot().map_err(host_error)?;
        if self.private_tabs(&snapshot).contains(&tab) {
            return Err(failure(
                ErrorCode::AuthFailed,
                "Uploads require a non-elevated terminal profile.",
            ));
        }
        let profile_id = snapshot
            .sessions
            .iter()
            .flat_map(|session| &session.tabs)
            .find(|saved| saved.id == tab)
            .map(|saved| saved.profile_id.as_str().to_owned())
            .ok_or_else(|| failure(ErrorCode::TargetLost, "This terminal is no longer running."))?;
        let profile = self
            .state
            .profiles
            .as_ref()
            .and_then(|profiles| profiles.profiles.iter().find(|p| p.id == profile_id));
        Ok(match profile {
            Some(wire::TerminalProfile {
                environment: wire::ProfileEnvironment::Wsl,
                distro: Some(distro),
                ..
            }) => UploadEnvironment::Wsl {
                distro: distro.clone(),
            },
            _ => UploadEnvironment::Local,
        })
    }
}

/// The inbox path as the WSL distro sees it, from the distro's own `wslpath`.
struct WslUploadPath;
impl files::UploadPathResolver for WslUploadPath {
    fn resolve(&self, native: &std::path::Path, actor: &UploadActor) -> files::Result<String> {
        let UploadEnvironment::Wsl { distro } = &actor.environment else {
            return Err(file_error(
                files::ErrorCode::Unsupported,
                "Not a WSL upload.",
            ));
        };
        let unavailable = || {
            file_error(
                files::ErrorCode::Unsupported,
                "WSL upload path could not be resolved.",
            )
        };
        #[cfg(not(windows))]
        {
            let _ = (native, distro);
            Err(unavailable())
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let wsl = shellfox_terminal::wsl::OsWsl::discover().map_err(|_| unavailable())?;
            let output = std::process::Command::new(wsl.path())
                .args([
                    "--distribution",
                    distro,
                    "--exec",
                    "/usr/bin/wslpath",
                    "-a",
                    "-u",
                ])
                .arg(native)
                .creation_flags(0x0800_0000)
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .map_err(|_| unavailable())?;
            let guest = String::from_utf8(output.stdout).map_err(|_| unavailable())?;
            let guest = guest
                .strip_suffix('\n')
                .unwrap_or(&guest)
                .trim_end_matches('\r');
            if !output.status.success()
                || !guest.starts_with('/')
                || guest.contains(['\r', '\n', '\0'])
            {
                return Err(unavailable());
            }
            Ok(guest.to_owned())
        }
    }
}
