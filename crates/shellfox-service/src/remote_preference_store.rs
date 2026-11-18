//! Trusted flat preference leaf. Same native cap/lost token and retained attempt.
use super::*;
use shellfox_files::{
    ErrorCode as FileCode, FileService, PrivateDurability, PrivatePublishOutcome,
    ScopedPrivatePublication,
};
use shellfox_protocol::CancellationToken;
use std::{
    ffi::OsStr,
    sync::{Arc, Mutex},
};
const LIMIT: usize = 256 * 1024;
struct State {
    attempt: Option<ScopedPrivatePublication>,
    quarantined: bool,
}
pub(in crate::remote) struct NativePreferenceStore {
    files: Arc<FileService>,
    lost: CancellationToken,
    state: Mutex<State>,
}
impl NativePreferenceStore {
    pub(in crate::remote) fn retained(files: Arc<FileService>, lost: CancellationToken) -> Self {
        Self {
            files,
            lost,
            state: Mutex::new(State {
                attempt: None,
                quarantined: false,
            }),
        }
    }
    fn owned(&self) -> Result<(), AppError> {
        if self.lost.is_cancelled() {
            Err(failed())
        } else {
            Ok(())
        }
    }
    fn read_bytes(&self) -> Result<Option<shellfox_files::PrivateRead>, AppError> {
        self.owned()?;
        let result = self
            .files
            .read_private_leaf(OsStr::new("remote-settings-v1.json"), LIMIT);
        self.owned()?;
        match result {
            Ok(read) => Ok(Some(read)),
            Err(error) if error.code == FileCode::NotFound => Ok(None),
            Err(_) => Err(failed()),
        }
    }
    pub(in crate::remote) fn read(&self) -> Result<Option<RemotePreferences>, AppError> {
        self.owned()?;
        let _state = self.state.lock().map_err(|_| failed())?;
        let Some(read) = self.read_bytes()? else {
            return Ok(None);
        };
        let value: RemotePreferences = serde_json::from_slice(&read.bytes).map_err(|_| failed())?;
        value.validate()?;
        self.owned()?;
        Ok(Some(value))
    }
    /// Success certifies visible publication only, not power-loss durability.
    /// Exact durability is retained as the returned native outcome, never commit authority.
    pub(in crate::remote) fn write(
        &self,
        value: RemotePreferences,
    ) -> Result<PrivateDurability, AppError> {
        value.validate()?;
        let bytes = serde_json::to_vec(&value).map_err(|_| failed())?;
        if bytes.len() > LIMIT {
            return Err(failed());
        }
        self.owned()?;
        let mut state = self.state.lock().map_err(|_| failed())?;
        if state.quarantined {
            return Err(failed());
        }
        let expected = self.read_bytes()?.map(|read| read.version);
        let stage = format!("remote-settings-{}.new", uuid::Uuid::new_v4().simple());
        // State/retention exist before the first possible native create.
        state.attempt = None;
        state.quarantined = true;
        match self.files.prepare_private_leaf_publication(
            OsStr::new("remote-settings-v1.json"),
            OsStr::new(&stage),
            expected.as_ref(),
        ) {
            Ok(attempt) => state.attempt = Some(attempt),
            Err(failure) => {
                let (_error, attempt) = failure.into_parts();
                state.attempt = attempt;
                state.quarantined = true;
                return Err(failed());
            }
        }
        self.owned()?;
        let attempt = state.attempt.as_mut().ok_or_else(failed)?;
        if attempt.write_all(&bytes).is_err() {
            state.quarantined = true;
            return Err(failed());
        }
        let outcome = attempt.publish();
        match outcome {
            PrivatePublishOutcome::Published(durability) => {
                // Persistent preferences are not ephemeral endpoint cleanup.
                // Close only our child FD; never unlink this published leaf.
                state.attempt.as_mut().ok_or_else(failed)?.preserve_stale();
                state.quarantined = false;
                Ok(durability)
            }
            PrivatePublishOutcome::NotPublished(_)
            | PrivatePublishOutcome::Uncertain(_)
            | PrivatePublishOutcome::Refused(_) => {
                state.quarantined = true;
                Err(failed())
            }
        }
    }
    pub(in crate::remote) fn preserve_retained(&self) -> Result<(), AppError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(attempt) = &mut state.attempt {
            attempt.verify_retained_parent().map_err(|_| failed())?;
            // No automatic deletion/retry of a failed or uncertain publication.
            attempt.preserve_stale();
        }
        Ok(())
    }
}
