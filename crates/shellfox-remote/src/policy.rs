use crate::{capability_unavailable, error};
use serde::{Deserialize, Serialize};
use shellfox_protocol::{AppError, AuthenticatedPeerId, ErrorCode, RemoteCommand};
use std::path::{Component, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViewPermission {
    /// Strict legacy record value. Current approved-device grants normalize to Write.
    ReadOnly,
    Write,
}

/// Host-local decision. Never deserialized from a remote request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Approval {
    pub peer_id: String,
    pub hostname: String,
    pub approved_at_millis: u64,
    pub permission: ViewPermission,
    /// Legacy private-record metadata only, never current filesystem authority.
    pub file_roots: Vec<PathBuf>,
}
impl Approval {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        let valid_root = |root: &PathBuf| {
            root.is_absolute()
                && shellfox_files::FileService::validate_legacy_root_syntax(root).is_ok()
                && root.to_str().is_some_and(|s| {
                    !s.is_empty() && s.len() <= 32767 && !s.chars().any(char::is_control)
                })
                && !root
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        };
        if AuthenticatedPeerId::new(self.peer_id.clone()).is_err()
            || self.hostname.is_empty()
            || self.hostname.len() > 253
            || self.hostname.chars().any(char::is_control)
            || self.approved_at_millis > shellfox_protocol::MAX_SAFE_INTEGER
            || self.file_roots.len() > 16
            || !self.file_roots.iter().all(valid_root)
        {
            return Err(error(
                ErrorCode::Validation,
                "Invalid approval record.",
                false,
            ));
        }
        Ok(())
    }
}

pub(crate) fn authorize(
    permission: ViewPermission,
    command: &RemoteCommand,
) -> Result<(), AppError> {
    use RemoteCommand::*;
    let read = matches!(
        command,
        GetSnapshot(_)
            | GetTerminalProfiles(_)
            | GetHomeDirectory(_)
            | ListFiles(_)
            | ReadFile(_)
            | BeginFileRead(_)
            | ReadFileChunk(_)
            | CancelFileTransfer(_)
            | ReadFileAsset(_)
            | GetSessionStats(_)
            | ResolveDirectory(_)
            | CompleteDirectory(_)
            | AcknowledgeTerminal(_)
            | DetachTerminal(_)
            | SetVisibleSessions(_)
    );
    if permission == ViewPermission::ReadOnly && !read {
        // Observer attachment needs a distinct runtime observer, never Claim.
        return Err(error(
            ErrorCode::AccessDenied,
            "This device has view-only permission.",
            false,
        ));
    }
    if matches!(command, AttachTerminal(_)) {
        return Err(capability_unavailable(
            "legacy full-state terminal recovery; retain Node compatibility host",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readonly_cannot_claim_or_mutate() {
        let command = serde_json::from_value::<RemoteCommand>(serde_json::json!({"method":"attachTerminal","payload":{"tabId":"11111111-1111-4111-8111-111111111111"}})).unwrap();
        assert_eq!(
            authorize(ViewPermission::ReadOnly, &command)
                .unwrap_err()
                .code,
            ErrorCode::AccessDenied
        );
        assert_eq!(
            authorize(ViewPermission::Write, &command).unwrap_err().code,
            ErrorCode::Unsupported
        );
        assert!(serde_json::from_value::<RemoteCommand>(
            serde_json::json!({"method":"setSettings","payload":{}})
        )
        .is_err());
    }
}
