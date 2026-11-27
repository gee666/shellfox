//! 0.9 file methods on the local host. Like the directory helpers they run in
//! the CLI process: it is the same user on the same machine. Relative paths
//! resolve against the session's (or tab's) folder, as for a peer.
use crate::{manager_local::Local, output::CliError};
use anyhow::Result;
use serde_json::{json, Value};
use shellfox_core::{SessionId, TabId};
use shellfox_files::{Access, FileService, Location};
use shellfox_remote::file_ops;
use std::path::Path;

/// The CLI's view of a files-crate failure, with the real message.
pub(crate) fn file_failure(error: shellfox_files::FileError) -> anyhow::Error {
    let code = match error.code {
        shellfox_files::ErrorCode::NotFound => "NOT_FOUND",
        shellfox_files::ErrorCode::Unsupported => "UNSUPPORTED",
        shellfox_files::ErrorCode::AccessDenied => "ACCESS_DENIED",
        shellfox_files::ErrorCode::StorageFailed => "STORAGE_FAILED",
        shellfox_files::ErrorCode::FileConflict => "FILE_CONFLICT",
        shellfox_files::ErrorCode::Validation => "VALIDATION",
    };
    CliError::coded(code, error.message, false)
}

impl Local<'_> {
    pub(crate) async fn file_path(
        &self,
        session: Option<SessionId>,
        tab: Option<TabId>,
        path: String,
    ) -> Result<String> {
        let Some(id) = session else {
            if tab.is_some() {
                return Err(CliError::usage("File tab requires a session."));
            }
            return Ok(path);
        };
        let requested = Path::new(&path);
        if requested.is_absolute() {
            return Ok(path);
        }
        if requested.has_root()
            || matches!(
                requested.components().next(),
                Some(std::path::Component::Prefix(_))
            )
        {
            return Err(CliError::usage(
                "File path needs an absolute drive or a session-relative path.",
            ));
        }
        let snapshot = self.snapshot().await?;
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| CliError::coded("NOT_FOUND", "File session not found.", false))?;
        let base = match tab {
            Some(id) => {
                &session
                    .tabs
                    .iter()
                    .find(|t| t.id == id)
                    .ok_or_else(|| {
                        CliError::coded("NOT_FOUND", "File tab does not belong to session.", false)
                    })?
                    .cwd
            }
            None => &session.cwd,
        };
        Path::new(base)
            .join(requested)
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| CliError::usage("File path is not UTF-8."))
    }

    async fn existing(
        &self,
        session: Option<SessionId>,
        tab: Option<TabId>,
        path: String,
    ) -> Result<(FileService, Location)> {
        let files = FileService::local_desktop(Access::ReadWrite);
        let path = self.file_path(session, tab, path).await?;
        let location = files
            .browser_existing(&Location { path })
            .map_err(file_failure)?;
        Ok((files, location))
    }

    pub(crate) async fn list_files(&self, input: shellfox_protocol::ListFiles) -> Result<Value> {
        let (files, location) = self
            .existing(input.session_id, input.tab_id, input.path)
            .await?;
        let list = files.list(&location).map_err(file_failure)?;
        file_ops::list_value(list).map_err(CliError::app)
    }

    pub(crate) async fn read_file(&self, input: shellfox_protocol::ReadFile) -> Result<Value> {
        let (files, location) = self
            .existing(input.session_id, input.tab_id, input.path)
            .await?;
        let read = files.read_file(&location).map_err(file_failure)?;
        file_ops::read_value(read).map_err(CliError::app)
    }

    pub(crate) async fn write_file(
        &self,
        input: shellfox_protocol::files::WriteFile,
    ) -> Result<Value> {
        let path = self
            .file_path(input.session_id, input.tab_id, input.path)
            .await?;
        let written = FileService::local_desktop(Access::ReadWrite)
            .write_text(&shellfox_files::WriteText {
                path,
                text: input.text,
                revision: input.revision,
            })
            .map_err(file_failure)?;
        Ok(json!({ "path": written.path, "revision": written.revision, "size": written.size }))
    }
}
