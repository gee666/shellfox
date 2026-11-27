//! Dispatch to the actual local session or original admitted peer connection. Never reinterpret a peer path locally.
use super::list::Target;
use anyhow::anyhow;
use anyhow::{Context as _, Result, ensure};
use shellfox_files::{
    AssetRequest, EditorBackend, EditorCancellation, EditorSession, EntryKind, ErrorCode,
    FileAsset, FileContext, FileList, FileService, FileStat, ImageWrite, Location, ReadResult,
    WriteResult, WriteText, WslBrowserScope,
};
use std::sync::Arc;
mod peer;
pub(crate) use peer::FileScope as PeerFileScope;
#[cfg(windows)]
mod wsl;
#[derive(Clone)]
pub(crate) enum DocumentBackend {
    Local(Arc<LocalDocumentBackend>),
    #[cfg(windows)]
    Wsl(Arc<wsl::WslBackend>),
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

    pub fn same_panel_scope(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local(_), Self::Local(_)) => true,
            (Self::Peer(a), Self::Peer(b)) => {
                a.ids() == b.ids()
                    && match (&a.scope, &b.scope) {
                        (None, None) => true,
                        (Some(a), Some(b)) => {
                            a.guest.distro().eq_ignore_ascii_case(b.guest.distro())
                        }
                        _ => false,
                    }
            }
            #[cfg(windows)]
            (Self::Wsl(a), Self::Wsl(b)) => {
                a.tab == b.tab
                    && a.account == b.account
                    && a.scope.distro().eq_ignore_ascii_case(b.scope.distro())
            }
            _ => false,
        }
    }

    pub fn is_wsl(&self) -> bool {
        self.wsl_scope().is_some()
    }

    pub fn wsl_scope(&self) -> Option<&WslBrowserScope> {
        match self {
            #[cfg(windows)]
            Self::Wsl(backend) => Some(&backend.scope),
            Self::Peer(peer) => peer.scope.as_ref().map(|scope| &scope.guest),
            Self::Local(_) => None,
        }
    }

    pub fn wsl_tab(&self) -> Option<shellfox_core::TabId> {
        match self {
            #[cfg(windows)]
            Self::Wsl(backend) => Some(backend.tab),
            _ => None,
        }
    }

    pub fn peer_file_scope(&self) -> Option<&PeerFileScope> {
        self.peer_backend().and_then(|peer| peer.scope.as_ref())
    }

    pub fn close_wsl_editor(&self) {
        #[cfg(windows)]
        if let Self::Wsl(backend) = self {
            backend.close();
        }
    }

    pub fn retire_wsl_editor(&self) {
        #[cfg(windows)]
        if let Self::Wsl(backend) = self {
            backend.retire();
        }
    }

    pub fn peer(
        binding: Arc<crate::host_client::peer_v1::V1Binding>,
        scope: Option<PeerFileScope>,
    ) -> Result<Self> {
        Ok(Self::Peer(peer::PeerDocumentBackend::new(binding, scope)?))
    }
    pub fn peer_backend(&self) -> Option<&Arc<peer::PeerDocumentBackend>> {
        match self {
            Self::Peer(peer) => Some(peer),
            _ => None,
        }
    }
    pub fn is_local(&self) -> bool {
        !matches!(self, Self::Peer(_))
    }
    pub fn check_current(&self) -> Result<()> {
        match self {
            Self::Peer(peer) => peer.check(),
            #[cfg(windows)]
            Self::Wsl(backend) => backend.check(),
            Self::Local(_) => Ok(()),
        }
    }
    pub fn same_origin(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local(a), Self::Local(b)) => Arc::ptr_eq(a, b),
            #[cfg(windows)]
            (Self::Wsl(a), Self::Wsl(b)) => Arc::ptr_eq(a, b),
            (Self::Peer(a), Self::Peer(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    pub fn commit_if_current<R>(&self, operation: impl FnOnce() -> R) -> Result<R> {
        match self {
            Self::Peer(peer) => peer.commit_if_current(operation),
            _ => Ok(operation()),
        }
    }
    pub async fn list(&self, path: &str) -> Result<FileList> {
        match self {
            Self::Local(local) => {
                let location = local.location(path)?;
                Ok(local.list_location(&location)?)
            }
            #[cfg(windows)]
            Self::Wsl(backend) => backend.list(path),
            Self::Peer(peer) => peer.list(path).await,
        }
    }
    pub(super) async fn complete(&self, path: &str) -> Result<Vec<String>> {
        if self.is_wsl() {
            if path.starts_with('~') {
                return Ok(Vec::new());
            }
            let (parent, prefix) = path.rsplit_once(['/', '\\']).unwrap_or((".", path));
            let parent = if parent.is_empty() { "/" } else { parent };
            let listing = self.list(parent).await?;
            return Ok(listing
                .entries
                .into_iter()
                .filter(|entry| {
                    entry.kind == EntryKind::Directory && entry.name.starts_with(prefix)
                })
                .take(30)
                .map(|entry| format!("{}/", entry.path))
                .collect());
        }
        match self {
            Self::Local(backend) => {
                let context = LocalDocumentBackend::home_context()?;
                Ok(backend
                    .service
                    .complete_directory_with_context(
                        &Location { path: path.into() },
                        Some(&context),
                    )?
                    .matches)
            }
            #[cfg(windows)]
            Self::Wsl(_) => Ok(Vec::new()),
            Self::Peer(peer) => peer.complete(path).await,
        }
    }
    pub(super) async fn lookup_path(&self, path: &str) -> Result<Target> {
        match self {
            Self::Local(backend) => backend.lookup_path(path),
            #[cfg(windows)]
            Self::Wsl(backend) => backend.lookup_path(path),
            Self::Peer(peer) => {
                let error = match peer.list(path).await {
                    Ok(listing) => return Ok(Target::Directory(listing)),
                    Err(error)
                        if error
                            .downcast_ref::<shellfox_files::FileError>()
                            .is_some_and(|e| {
                                matches!(e.code, ErrorCode::Validation | ErrorCode::NotFound)
                            }) =>
                    {
                        error
                    }
                    Err(error) => return Err(error),
                };
                let (parent, name) = split_peer_path(path);
                match peer.list(parent).await {
                    Ok(parent) => {
                        let windows = path.as_bytes().get(1) == Some(&b':');
                        let entry = parent.entries.iter().find(|item| {
                            item.name == name
                                || windows && item.name.to_lowercase() == name.to_lowercase()
                        });
                        if let Some(entry) = entry {
                            if matches!(entry.kind, EntryKind::File | EntryKind::Symlink) {
                                return Ok(Target::File {
                                    path: entry.path.clone(),
                                    parent,
                                });
                            }
                        } else if parent.truncated {
                            anyhow::bail!(
                                "Folder listing is incomplete. Choose the file from its folder."
                            );
                        }
                    }
                    Err(parent_error)
                        if parent_error
                            .downcast_ref::<shellfox_files::FileError>()
                            .is_some_and(|e| e.code == ErrorCode::NotFound) => {}
                    Err(parent_error) => return Err(parent_error),
                }
                if error
                    .downcast_ref::<shellfox_files::FileError>()
                    .is_some_and(|e| e.code == ErrorCode::NotFound)
                {
                    // Name the folder that was searched, as a copyable absolute path.
                    let searched = peer.host_path(path).await;
                    Ok(Target::Missing(searched.unwrap_or_else(|_| path.into())))
                } else {
                    Err(error)
                }
            }
        }
    }
    pub(super) async fn create_directory(&self, path: &str) -> Result<FileList> {
        self.check_current()?;
        match self {
            Self::Local(backend) => {
                let location = backend
                    .service
                    .browser_create_target(&backend.location(path)?)?;
                backend
                    .service
                    .create_directory_with_context(&location, None)?;
                Ok(backend.list_location(&location)?)
            }
            #[cfg(windows)]
            Self::Wsl(backend) => backend.create_directory(path),
            Self::Peer(peer) => peer.create_directory(path).await,
        }
    }
    pub async fn read_asset(
        &self,
        path: &str,
        asset: String,
        cancel: EditorCancellation,
    ) -> Result<FileAsset> {
        match self {
            Self::Local(local) => Ok(local.session.read_asset(
                &AssetRequest {
                    path: local.service.browser_existing(&local.location(path)?)?.path,
                    asset_path: asset,
                },
                cancel,
            )?),
            #[cfg(windows)]
            Self::Wsl(backend) => backend.asset(path, asset, cancel),
            Self::Peer(peer) => peer.read_asset(path, asset, &cancel).await,
        }
    }

    pub async fn read(&self, path: &str, cancel: &EditorCancellation) -> Result<ReadResult> {
        match self {
            Self::Local(local) => local.read_document(path, cancel),
            #[cfg(windows)]
            Self::Wsl(backend) => backend.read(path, cancel),
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
            #[cfg(windows)]
            Self::Wsl(backend) => backend.save(input, cancel),
            Self::Peer(peer) => peer.write(input, cancel).await,
        }
    }
    pub async fn write_image(
        &self,
        input: &ImageWrite,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        match self {
            Self::Local(local) => Ok(local.session.write_image(input, cancel)?),
            #[cfg(windows)]
            Self::Wsl(backend) => backend.save_image(input, cancel),
            Self::Peer(peer) => peer.write_image(input, cancel).await,
        }
    }

    pub fn local_stat_for_explorer(&self, path: &str) -> Result<FileStat> {
        ensure!(
            self.is_local(),
            "A peer path cannot be opened in this machine's file explorer."
        );
        match self {
            Self::Local(local) => Ok(local
                .service
                .stat(&local.service.browser_existing(&local.location(path)?)?)?),
            #[cfg(windows)]
            Self::Wsl(_) => Err(anyhow!(
                "Open the path in the WSL terminal. WSL Files does not use Windows Explorer."
            )),
            Self::Peer(_) => Err(anyhow!("Peer explorer operation refused.")),
        }
    }
}

fn split_peer_path(path: &str) -> (&str, &str) {
    path.rfind(['/', '\\']).map_or((".", path), |index| {
        let root = index == 0 || index == 2 && path.as_bytes().get(1) == Some(&b':');
        (&path[..index + usize::from(root)], &path[index + 1..])
    })
}

#[cfg(test)]
mod tests;

mod create;
mod local;
