//! Peer-only document I/O, bound to the V1 connection it was opened on.
use crate::host_client::peer_v1::V1Binding;
use anyhow::{Result, anyhow, ensure};
use shellfox_files::{
    EditorCancellation, FileList, ImageWrite, ReadResult, WriteResult, WriteText,
};
use shellfox_protocol as wire;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
mod read;
mod requests;
#[cfg(test)]
mod tests;
mod write;
use requests::file_error;

const SLOW_REQUEST: std::time::Duration = std::time::Duration::from_secs(2);
#[derive(Clone, Copy)]
enum SaveState {
    Active,
    Unconfirmed,
}

#[derive(Clone)]
pub(crate) struct FileScope {
    pub session: wire::SessionId,
    pub tab: wire::TabId,
    pub guest: shellfox_files::WslBrowserScope,
}

pub(crate) struct PeerDocumentBackend {
    binding: Arc<V1Binding>,
    /// Requests fail once the binding reconnects; reopen the document then.
    connection_id: u64,
    pub scope: Option<FileScope>,
    saves: Mutex<BTreeMap<String, SaveState>>,
}
impl PeerDocumentBackend {
    pub fn new(binding: Arc<V1Binding>, scope: Option<FileScope>) -> Result<Arc<Self>> {
        let connection_id = binding.state().connection_id;
        binding.with_connection(connection_id, || ())?;
        Ok(Arc::new(Self {
            binding,
            connection_id,
            scope,
            saves: Mutex::new(BTreeMap::new()),
        }))
    }
    pub fn ids(&self) -> (Option<wire::SessionId>, Option<wire::TabId>) {
        self.scope
            .as_ref()
            .map_or((None, None), |scope| (Some(scope.session), Some(scope.tab)))
    }

    pub fn check(&self) -> Result<()> {
        self.commit_if_current(|| ())
    }
    pub fn commit_if_current<R>(&self, operation: impl FnOnce() -> R) -> Result<R> {
        self.binding.with_connection(self.connection_id, operation)
    }
    async fn execute<R: wire::Request>(&self, request: &R) -> Result<R::Response> {
        let started = std::time::Instant::now();
        let reply = self
            .binding
            .v1_reply_with_connection(self.connection_id, request)
            .await;
        // Files waits on every reply with its controls disabled, so a slow
        // peer is worth a line in the UI log.
        let elapsed = started.elapsed();
        if elapsed >= SLOW_REQUEST {
            eprintln!(
                "Peer file request {} took {:.1} s ({})",
                R::METHOD,
                elapsed.as_secs_f64(),
                if reply.is_ok() { "answered" } else { "failed" }
            );
        }
        match reply? {
            wire::Reply::Success(value) => Ok(value),
            wire::Reply::Failure(error) => Err(file_error(error.code, error.message)),
        }
    }
    /// For transfer chunks only: cancel abandons the chunk in flight, and the
    /// caller then cancels the transfer so the peer discards it.
    async fn execute_cancellable<R: wire::Request>(
        &self,
        request: &R,
        cancel: &EditorCancellation,
    ) -> Result<R::Response> {
        until_cancelled(cancel, self.execute(request)).await
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
        let path = &self.host_path(path).await?;
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
    /// Expands `~` and `~/…` to the host home for an unscoped peer; other paths pass through.
    pub(super) async fn host_path(&self, path: &str) -> Result<String> {
        if self.scope.is_some() || !(path == "~" || path.starts_with("~/")) {
            return Ok(path.into());
        }
        let home = self.home().await?;
        Ok(if let Some(tail) = path.strip_prefix("~/") {
            let sep = if home.contains('\\') { '\\' } else { '/' };
            format!(
                "{}{sep}{}",
                home.trim_end_matches(['/', '\\']),
                tail.replace('/', &sep.to_string())
            )
        } else {
            home
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
    fn begin_save(&self, path: &str) -> Result<SaveAttempt<'_>> {
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
/// Drops `work` as soon as `cancel` fires.
async fn until_cancelled<T>(
    cancel: &EditorCancellation,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(anyhow!("File operation cancelled.")),
        result = work => result,
    }
}
struct SaveAttempt<'a> {
    backend: &'a PeerDocumentBackend,
    path: String,
    confirmed: bool,
}
impl SaveAttempt<'_> {
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
impl Drop for SaveAttempt<'_> {
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
