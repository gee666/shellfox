//! Peer-only document I/O. The original opaque Manager admission is never replaced by a UI cycle.
use crate::host_client::{
    peer::{PeerConnection, SourceAdmission, Transport},
    peer_v1::V1Binding,
};
use anyhow::{Result, anyhow, ensure};
use shellfox_files::{
    EditorCancellation, FileList, ImageWrite, ReadResult, WriteResult, WriteText,
};
use shellfox_protocol::{self as wire, OutboundTransport, manager_v2 as m};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[path = "peer_document_read.rs"]
mod read;
#[path = "files_peer_requests.rs"]
mod requests;
#[path = "peer_document_write.rs"]
mod write;
use requests::{FileRequest, file_error};
#[derive(Clone, Copy)]
enum SaveState {
    Active,
    Unconfirmed,
}
enum Origin<T: OutboundTransport> {
    Manager(Arc<PeerConnection<T>>, SourceAdmission),
    Legacy(Arc<V1Binding>, u64),
}

#[derive(Clone)]
pub(crate) struct FileScope {
    pub session: wire::SessionId,
    pub tab: wire::TabId,
    pub guest: shellfox_files::WslBrowserScope,
}

pub(crate) struct PeerDocumentBackend<T: OutboundTransport = Transport> {
    origin: Origin<T>,
    pub scope: Option<FileScope>,
    saves: Mutex<BTreeMap<String, SaveState>>,
}
impl<T: OutboundTransport> PeerDocumentBackend<T> {
    pub fn new(
        connection: Arc<PeerConnection<T>>,
        admission: SourceAdmission,
        scope: Option<FileScope>,
    ) -> Result<Arc<Self>> {
        admission.check()?;
        ensure!(
            admission.peer() == connection.client().peer(),
            "Document admission belongs to another peer."
        );
        Ok(Arc::new(Self {
            origin: Origin::Manager(connection, admission),
            scope,
            saves: Mutex::new(BTreeMap::new()),
        }))
    }
    pub fn legacy(binding: Arc<V1Binding>, scope: Option<FileScope>) -> Result<Arc<Self>> {
        let connection_id = binding.state().connection_id;
        binding.with_connection(connection_id, || ())?;
        Ok(Arc::new(Self {
            origin: Origin::Legacy(binding, connection_id),
            scope,
            saves: Mutex::new(BTreeMap::new()),
        }))
    }
    pub fn ids(&self) -> (Option<wire::SessionId>, Option<wire::TabId>) {
        self.scope.as_ref().map_or((None, None), |scope| (Some(scope.session), Some(scope.tab)))
    }

    pub fn check(&self) -> Result<()> {
        self.commit_if_current(|| ())
    }
    pub fn commit_if_current<R>(&self, operation: impl FnOnce() -> R) -> Result<R> {
        match &self.origin {
            Origin::Manager(_, admission) => admission.commit_if_current(operation),
            Origin::Legacy(binding, id) => binding.with_connection(*id, operation),
        }
    }
    async fn execute<R: FileRequest>(&self, request: &R) -> Result<R::Response> {
        match &self.origin {
            Origin::Manager(connection, admission) => match connection
                .execute_admitted_reply(admission, &request.manager())
                .await?
            {
                m::Reply::Success(value) => Ok(value),
                m::Reply::Failure(error) => Err(file_error(error.code, error.message)),
            },
            Origin::Legacy(binding, id) => {
                match binding.v1_reply_with_connection(*id, request).await? {
                    wire::Reply::Success(value) => Ok(value),
                    wire::Reply::Failure(error) => Err(file_error(error.code, error.message)),
                }
            }
        }
    }
    fn check_cancel(&self, cancel: &EditorCancellation) -> Result<()> {
        self.check()?;
        ensure!(!cancel.is_cancelled(), "File operation cancelled.");
        Ok(())
    }
    pub async fn home(&self) -> Result<String> {
        let home = self.execute(&wire::GetHomeDirectory {}).await?;
        wire::files::path(&home.cwd).map_err(|_| anyhow!("Invalid peer home response."))?;
        self.check()?;
        Ok(home.cwd)
    }
    pub async fn complete(&self, path: &str) -> Result<Vec<String>> {
        Ok(self
            .execute(&wire::CompleteDirectory { path: path.into() })
            .await?
            .matches)
    }
    pub async fn list(&self, path: &str) -> Result<FileList> {
        let (session_id, tab_id) = self.ids();
        let host_home;
        let path = if self.scope.is_none() && (path == "~" || path.starts_with("~/")) {
            let home = self.home().await?;
            host_home = if let Some(tail) = path.strip_prefix("~/") {
                let sep = if home.contains('\\') { '\\' } else { '/' };
                format!("{}{sep}{}", home.trim_end_matches(['/', '\\']), tail.replace('/', &sep.to_string()))
            } else { home };
            &host_home
        } else { path };
        wire::files::path(path).map_err(|_| anyhow!("Invalid peer folder path."))?;
        let list = self
            .execute(&wire::ListFiles {
                session_id,
                tab_id,
                path: path.into(),
            })
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
    pub async fn create_directory(&self, path: &str) -> Result<FileList> {
        let (session_id, tab_id) = self.ids();
        let created = self
            .execute(&wire::CreateDirectory {
                path: path.into(),
                session_id,
                tab_id,
            })
            .await?;
        self.list(&created.cwd).await
    }
    async fn cancel_once(&self, request: wire::CancelFileTransfer) {
        // Cleanup can use only the ORIGINAL admission/transfer. Never reacquire, pair or retry.
        let _ = self.execute(&request).await;
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
