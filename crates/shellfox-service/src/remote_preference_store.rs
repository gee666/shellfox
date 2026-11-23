//! Remote preferences, one private leaf in the native control directory.
use super::*;
use shellfox_files::{ErrorCode as FileCode, FileService};
use shellfox_protocol::CancellationToken;
use std::{
    ffi::OsStr,
    sync::{Arc, Mutex},
};
const LEAF: &str = "remote-settings-v1.json";
const LIMIT: usize = 256 * 1024;
pub(in crate::remote) struct NativePreferenceStore {
    files: Arc<FileService>,
    lost: CancellationToken,
    /// Serializes read-modify-write so two writers cannot both pass the version check.
    state: Mutex<()>,
}
impl NativePreferenceStore {
    pub(in crate::remote) fn retained(files: Arc<FileService>, lost: CancellationToken) -> Self {
        Self {
            files,
            lost,
            state: Mutex::new(()),
        }
    }
    fn owned(&self) -> Result<(), AppError> {
        if self.lost.is_cancelled() {
            Err(failed())
        } else {
            Ok(())
        }
    }
    fn read_bytes(&self) -> Result<Option<shellfox_files::private::PrivateRead>, AppError> {
        self.owned()?;
        let result = self.files.read_private(OsStr::new(LEAF), LIMIT);
        self.owned()?;
        match result {
            Ok(read) => Ok(Some(read)),
            Err(error) if error.code == FileCode::NotFound => Ok(None),
            Err(_) => Err(failed()),
        }
    }
    pub(in crate::remote) fn read(&self) -> Result<Option<RemotePreferences>, AppError> {
        let _state = self.state.lock().map_err(|_| failed())?;
        let Some(read) = self.read_bytes()? else {
            return Ok(None);
        };
        let value: RemotePreferences = serde_json::from_slice(&read.bytes).map_err(|_| failed())?;
        value.validate()?;
        self.owned()?;
        Ok(Some(value))
    }
    /// Replaces the stored preferences atomically. A concurrent external change is a failure.
    pub(in crate::remote) fn write(&self, value: RemotePreferences) -> Result<(), AppError> {
        value.validate()?;
        let bytes = serde_json::to_vec(&value).map_err(|_| failed())?;
        if bytes.len() > LIMIT {
            return Err(failed());
        }
        let _state = self.state.lock().map_err(|_| failed())?;
        let expected = self.read_bytes()?.map(|read| read.version);
        self.owned()?;
        self.files
            .write_private(OsStr::new(LEAF), &bytes, expected.as_ref())
            .map_err(|_| failed())?;
        Ok(())
    }
}
