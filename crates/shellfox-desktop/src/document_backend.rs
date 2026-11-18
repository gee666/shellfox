//! Dispatch to the actual local session or original admitted peer connection. Never reinterpret a peer path locally.
use anyhow::{Result, anyhow, ensure};
use shellfox_files::{
    EditorBackend, EditorCancellation, EditorSession, FileList, FileService, FileStat, Location,
    TextFile, WriteResult, WriteText,
};
use std::sync::Arc;
#[cfg(feature = "experimental-shellfox-native")]
#[path = "peer_document_backend.rs"]
mod peer;
#[derive(Clone)]
pub(crate) enum DocumentBackend {
    Local(Arc<LocalDocumentBackend>),
    #[cfg(feature = "experimental-shellfox-native")]
    Peer(Arc<peer::PeerDocumentBackend>),
}
pub(crate) struct LocalDocumentBackend {
    service: Arc<FileService>,
    session: Arc<EditorSession>,
}
impl DocumentBackend {
    pub fn local(backend: &Arc<EditorBackend>) -> Self {
        Self::Local(Arc::new(LocalDocumentBackend {
            service: backend.capability(),
            session: backend.session(),
        }))
    }
    #[cfg(feature = "experimental-shellfox-native")]
    pub fn peer(
        connection: Arc<crate::host_client::peer::Connection>,
        admission: crate::host_client::peer::SourceAdmission,
    ) -> Result<Self> {
        Ok(Self::Peer(peer::PeerDocumentBackend::new(
            connection, admission,
        )?))
    }
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
    pub fn check_current(&self) -> Result<()> {
        match self {
            Self::Local(_) => Ok(()),
            #[cfg(feature = "experimental-shellfox-native")]
            Self::Peer(peer) => peer.check(),
        }
    }
    pub fn same_origin(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local(a), Self::Local(b)) => Arc::ptr_eq(a, b),
            #[cfg(feature = "experimental-shellfox-native")]
            (Self::Peer(a), Self::Peer(b)) => Arc::ptr_eq(a, b),
            #[cfg(feature = "experimental-shellfox-native")]
            _ => false,
        }
    }
    pub fn commit_if_current<R>(&self, operation: impl FnOnce() -> R) -> Result<R> {
        match self {
            Self::Local(_) => Ok(operation()),
            #[cfg(feature = "experimental-shellfox-native")]
            Self::Peer(peer) => peer.commit_if_current(operation),
        }
    }
    pub async fn initial_path(&self, preferred: Option<String>) -> Result<String> {
        #[cfg(not(feature = "experimental-shellfox-native"))]
        let _ = preferred;
        match self {
            Self::Local(_) => std::env::current_dir()?
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("Current local folder is not UTF-8.")),
            #[cfg(feature = "experimental-shellfox-native")]
            Self::Peer(peer) => match preferred {
                Some(path) => {
                    shellfox_protocol::files::path(&path)
                        .map_err(|_| anyhow!("Invalid original peer folder selector."))?;
                    peer.check()?;
                    Ok(path)
                }
                None => peer.home().await,
            },
        }
    }
    pub async fn list(&self, path: &str) -> Result<FileList> {
        match self {
            Self::Local(local) => Ok(local.service.list(&Location { path: path.into() })?),
            #[cfg(feature = "experimental-shellfox-native")]
            Self::Peer(peer) => peer.list(path).await,
        }
    }
    pub async fn read(&self, path: &str, cancel: &EditorCancellation) -> Result<TextFile> {
        match self {
            Self::Local(local) => Ok(local
                .session
                .read(&Location { path: path.into() }, cancel)?),
            #[cfg(feature = "experimental-shellfox-native")]
            Self::Peer(peer) => peer.read(path, cancel).await,
        }
    }
    pub async fn write(
        &self,
        input: &WriteText,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        match self {
            Self::Local(local) => Ok(local.session.write(input, cancel)?),
            #[cfg(feature = "experimental-shellfox-native")]
            Self::Peer(peer) => peer.write(input, cancel).await,
        }
    }
    pub fn local_stat_for_explorer(&self, path: &str) -> Result<FileStat> {
        ensure!(
            self.is_local(),
            "A peer path cannot be opened in this machine's file explorer."
        );
        match self {
            Self::Local(local) => Ok(local.service.stat(&Location { path: path.into() })?),
            #[cfg(feature = "experimental-shellfox-native")]
            Self::Peer(_) => Err(anyhow!("Peer explorer operation refused.")),
        }
    }
}
