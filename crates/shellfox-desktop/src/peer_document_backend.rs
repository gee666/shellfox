//! Peer-only document I/O. The original opaque Manager admission is never replaced by a UI cycle.
use crate::host_client::peer::{PeerConnection, SourceAdmission, Transport};
use anyhow::{Result, anyhow, ensure};
use shellfox_files::{EditorCancellation, FileList, TextFile, WriteResult, WriteText};
use shellfox_protocol::{self as wire, OutboundTransport, manager_v2 as m};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[path = "peer_document_read.rs"]
mod read;
#[path = "peer_document_write.rs"]
mod write;
#[derive(Clone, Copy)]
enum SaveState {
    Active,
    Unconfirmed,
}
pub(crate) struct PeerDocumentBackend<T: OutboundTransport = Transport> {
    connection: Arc<PeerConnection<T>>,
    admission: SourceAdmission,
    saves: Mutex<BTreeMap<String, SaveState>>,
}
impl<T: OutboundTransport> PeerDocumentBackend<T> {
    pub fn new(
        connection: Arc<PeerConnection<T>>,
        admission: SourceAdmission,
    ) -> Result<Arc<Self>> {
        admission.check()?;
        ensure!(
            admission.peer() == connection.client().peer(),
            "Document admission belongs to another peer."
        );
        Ok(Arc::new(Self {
            connection,
            admission,
            saves: Mutex::new(BTreeMap::new()),
        }))
    }
    pub fn check(&self) -> Result<()> {
        self.admission.check()
    }
    pub fn commit_if_current<R>(&self, operation: impl FnOnce() -> R) -> Result<R> {
        self.admission.commit_if_current(operation)
    }
    async fn execute<R: m::Request>(&self, request: &R) -> Result<R::Response> {
        self.connection
            .execute_admitted(&self.admission, request)
            .await
    }
    fn check_cancel(&self, cancel: &EditorCancellation) -> Result<()> {
        self.check()?;
        ensure!(!cancel.is_cancelled(), "File operation cancelled.");
        Ok(())
    }
    pub async fn home(&self) -> Result<String> {
        let home = self
            .execute(&m::GetHomeDirectory(wire::GetHomeDirectory {}))
            .await?;
        wire::files::path(&home.cwd).map_err(|_| anyhow!("Invalid peer home response."))?;
        self.check()?;
        Ok(home.cwd)
    }
    pub async fn list(&self, path: &str) -> Result<FileList> {
        wire::files::path(path).map_err(|_| anyhow!("Invalid peer folder path."))?;
        let list = self
            .execute(&m::ListFiles(wire::ListFiles {
                session_id: None,
                tab_id: None,
                path: path.into(),
            }))
            .await?;
        let entries = list
            .entries
            .into_iter()
            .map(|entry| shellfox_files::FileEntry {
                name: entry.name,
                path: entry.path,
                kind: match entry.kind {
                    wire::files::EntryKind::File => shellfox_files::EntryKind::File,
                    wire::files::EntryKind::Directory => shellfox_files::EntryKind::Directory,
                    wire::files::EntryKind::Symlink => shellfox_files::EntryKind::Symlink,
                    wire::files::EntryKind::Other => shellfox_files::EntryKind::Other,
                },
            })
            .collect();
        self.check()?;
        Ok(FileList {
            path: list.path,
            parent_path: list.parent_path,
            entries,
            truncated: list.truncated,
        })
    }
    async fn cancel_once(&self, request: wire::CancelFileTransfer) {
        // Cleanup can use only the ORIGINAL admission/transfer. Never reacquire, pair or retry.
        let _ = self.execute(&m::CancelFileTransfer(request)).await;
    }
    fn begin_save(&self, path: &str) -> Result<SaveAttempt<'_, T>> {
        let mut saves = self
            .saves
            .lock()
            .map_err(|_| anyhow!("Peer save state is unavailable."))?;
        ensure!(
            !saves.contains_key(path),
            "Read this peer document again before another save attempt."
        );
        ensure!(
            saves.len() < 32,
            "Too many unconfirmed peer saves. Reload their documents first."
        );
        saves.insert(path.into(), SaveState::Active);
        Ok(SaveAttempt {
            backend: self,
            path: path.into(),
            confirmed: false,
        })
    }
    fn read_confirmed(&self, requested: &str, canonical: &str) -> Result<()> {
        let mut saves = self
            .saves
            .lock()
            .map_err(|_| anyhow!("Peer save state is unavailable."))?;
        for path in [requested, canonical] {
            if matches!(saves.get(path), Some(SaveState::Unconfirmed)) {
                saves.remove(path);
            }
        }
        Ok(())
    }
}
struct SaveAttempt<'a, T: OutboundTransport> {
    backend: &'a PeerDocumentBackend<T>,
    path: String,
    confirmed: bool,
}
impl<T: OutboundTransport> SaveAttempt<'_, T> {
    fn confirm(mut self) -> Result<()> {
        self.backend
            .saves
            .lock()
            .map_err(|_| anyhow!("Peer save state is unavailable."))?
            .remove(&self.path);
        self.confirmed = true;
        Ok(())
    }
}
impl<T: OutboundTransport> Drop for SaveAttempt<'_, T> {
    fn drop(&mut self) {
        if !self.confirmed {
            let mut saves = self
                .backend
                .saves
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            saves.insert(self.path.clone(), SaveState::Unconfirmed);
        }
    }
}
