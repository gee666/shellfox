use super::{DocumentBackend, Target};
use anyhow::{Context, Result, anyhow, bail, ensure};
use shellfox_core::{TabId, WslLaunchAccount};
use shellfox_files::{
    EditorCancellation, EntryKind, ErrorCode, FileAsset, FileError, FileList, ImageWrite, Location,
    ReadResult, WriteResult, WriteText, WslBrowserScope,
    wsl_files_ipc::{Reply, Request},
};
use shellfox_service::wsl_files_worker::Worker;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

mod create;

pub(crate) struct WslBackend {
    pub(super) scope: WslBrowserScope,
    pub(super) tab: TabId,
    open: Mutex<Option<Worker>>,
    retired: AtomicBool,
    pub(super) account: WslLaunchAccount,
}

impl DocumentBackend {
    pub(crate) fn local_wsl(scope: WslBrowserScope, tab: TabId, account: WslLaunchAccount) -> Self {
        Self::Wsl(Arc::new(WslBackend {
            scope,
            tab,
            open: Mutex::new(None),
            retired: AtomicBool::new(false),
            account,
        }))
    }
}

impl WslBackend {
    fn location(&self, path: &str) -> Result<Location> {
        Ok(Location {
            path: self.scope.guest(path)?,
        })
    }

    pub(super) fn check(&self) -> Result<()> {
        ensure!(
            !self.retired.load(Ordering::Acquire),
            "The WSL tab has closed."
        );
        Ok(())
    }

    fn start(&self) -> Result<Worker> {
        self.check()?;
        let worker = Worker::start(self.scope.distro(), &self.account)?;
        self.check()?;
        Ok(worker)
    }

    fn call(&self, request: Request) -> Result<Reply> {
        Self::without_payload(self.start()?.exchange(request, Vec::new())?)
    }

    fn without_payload(packet: (Reply, Vec<u8>)) -> Result<Reply> {
        ensure!(packet.1.is_empty(), "Unexpected WSL Files payload");
        match packet.0 {
            Reply::Error(error) => Err(error.into()),
            reply => Ok(reply),
        }
    }

    pub(super) fn list(&self, path: &str) -> Result<FileList> {
        self.list_reply(self.call(Request::List(self.location(path)?))?)
    }

    pub(super) fn create_directory(&self, path: &str) -> Result<FileList> {
        self.list_reply(self.call(Request::CreateDirectory(self.location(path)?))?)
    }

    fn list_reply(&self, reply: Reply) -> Result<FileList> {
        let Reply::List(list) = reply else {
            bail!("Unexpected WSL directory response")
        };
        self.expected_path(&list.path)?;
        if let Some(parent) = &list.parent_path {
            self.expected_path(parent)?;
        }
        ensure!(
            list.entries.len() <= shellfox_files::LIST_LIMIT,
            "WSL Files listing exceeds its limit"
        );
        for entry in &list.entries {
            self.expected_path(&entry.path)?;
            ensure!(
                !entry.name.contains('/')
                    && entry.path == format!("{}/{}", list.path.trim_end_matches('/'), entry.name),
                "Entry is outside its WSL directory"
            );
        }
        Ok(list)
    }

    fn expected_path(&self, path: &str) -> Result<()> {
        ensure!(
            path.starts_with('/') && self.scope.guest(path)? == path,
            "Invalid path returned by WSL Files"
        );
        Ok(())
    }

    pub(super) fn lookup_path(&self, path: &str) -> Result<Target> {
        let location = self.location(path)?;
        let worker = self.start()?;
        let call = |request| Self::without_payload(worker.exchange(request, Vec::new())?);
        match call(Request::List(location.clone())).and_then(|reply| self.list_reply(reply)) {
            Ok(list) => return Ok(Target::Directory(list)),
            Err(error) if missing(&error) => {}
            Err(error) if file_error(&error, ErrorCode::Validation) => {
                if !matches!(call(Request::Stat(location.clone())), Ok(Reply::Stat(stat)) if stat.kind == EntryKind::File)
                {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        let Some((parent, name)) = location.path.rsplit_once('/') else {
            bail!("Expected a guest path")
        };
        let parent = Location {
            path: if parent.is_empty() { "/" } else { parent }.into(),
        };
        match call(Request::List(parent)).and_then(|reply| self.list_reply(reply)) {
            Ok(parent) => {
                if let Some(file) = parent.entries.iter().find(|entry| entry.name == name) {
                    if matches!(file.kind, EntryKind::File | EntryKind::Symlink) {
                        let path = file.path.clone();
                        return Ok(Target::File { parent, path });
                    }
                } else if parent.truncated {
                    bail!("Folder listing is incomplete. Choose the file from its folder.");
                }
            }
            Err(error) if missing(&error) => {}
            Err(error) => return Err(error),
        }
        Ok(Target::Missing(location.path))
    }

    pub(super) fn read(&self, path: &str, cancel: &EditorCancellation) -> Result<ReadResult> {
        let location = self.location(path)?;
        let mut open = self
            .open
            .lock()
            .map_err(|_| anyhow!("WSL Files is unavailable"))?;
        open.take();
        let worker = self.start()?;
        let (reply, bytes) = worker.exchange(Request::Open(location), Vec::new())?;
        let mut document = match reply {
            Reply::Document(document) => document,
            Reply::Error(error) => return Err(error.into()),
            _ => bail!("Unexpected WSL file response"),
        };
        match &mut document {
            ReadResult::Text {
                path, size, text, ..
            } => {
                self.expected_path(path)?;
                ensure!(
                    text.is_empty()
                        && *size == bytes.len() as u64
                        && *size <= shellfox_files::TRANSFER_TEXT_BYTES,
                    "Invalid WSL text response"
                );
                *text = String::from_utf8(bytes)?;
                ensure!(!text.contains('\0'), "Invalid WSL text");
            }
            ReadResult::Image {
                path,
                size,
                bytes: data,
                ..
            } => {
                self.expected_path(path)?;
                ensure!(
                    data.is_empty()
                        && *size == bytes.len() as u64
                        && *size <= shellfox_files::TRANSFER_IMAGE_BYTES,
                    "Invalid WSL image response"
                );
                *data = bytes;
            }
            ReadResult::Binary { path, .. } => {
                self.expected_path(path)?;
                ensure!(bytes.is_empty(), "Unexpected WSL binary payload");
            }
        }
        ensure!(!cancel.is_cancelled(), "File operation cancelled");
        self.check()?;
        if !matches!(document, ReadResult::Binary { .. }) {
            *open = Some(worker);
        }
        Ok(document)
    }

    pub(super) fn save(
        &self,
        input: &WriteText,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        ensure!(
            input.text.len() as u64 <= shellfox_files::TRANSFER_TEXT_BYTES,
            "Text exceeds the 8 MiB editor limit"
        );
        let path = self.location(&input.path)?.path;
        self.save_packet(
            Request::Save(WriteText {
                path: path.clone(),
                revision: input.revision.clone(),
                text: String::new(),
            }),
            input.text.as_bytes().to_vec(),
            &path,
            cancel,
        )
    }

    pub(super) fn save_image(
        &self,
        input: &ImageWrite,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        ensure!(
            input.bytes.len() as u64 <= shellfox_files::TRANSFER_IMAGE_BYTES,
            "Image exceeds the 64 MiB editor limit"
        );
        let path = self.location(&input.path)?.path;
        self.save_packet(
            Request::SaveImage(ImageWrite {
                path: path.clone(),
                revision: input.revision.clone(),
                mime_type: input.mime_type.clone(),
                bytes: Vec::new(),
            }),
            input.bytes.clone(),
            &path,
            cancel,
        )
    }

    fn save_packet(
        &self,
        request: Request,
        bytes: Vec<u8>,
        path: &str,
        cancel: &EditorCancellation,
    ) -> Result<WriteResult> {
        let size = bytes.len() as u64;
        let mut open = self
            .open
            .lock()
            .map_err(|_| anyhow!("WSL Files is unavailable"))?;
        ensure!(!cancel.is_cancelled(), "File operation cancelled");
        self.check()?;
        let worker = open
            .as_ref()
            .context("The WSL editor has closed. Reload before saving.")?;
        // Once sent, wait for the original worker rather than retrying or cancelling publication.
        let result = worker
            .exchange(request, bytes)
            .and_then(Self::without_payload);
        if self.check().is_err() {
            open.take();
        }
        match result {
            Ok(Reply::Saved(result)) if result.path == path && result.size == size => Ok(result),
            Err(error) if error.downcast_ref::<FileError>().is_some() => Err(error),
            _ => {
                open.take();
                bail!("WSL save was not confirmed. Reload before saving again.");
            }
        }
    }

    pub(super) fn asset(
        &self,
        path: &str,
        asset: String,
        cancel: EditorCancellation,
    ) -> Result<FileAsset> {
        let input = shellfox_files::AssetRequest {
            path: self.location(path)?.path,
            asset_path: asset,
        };
        let (reply, bytes) = self.start()?.exchange(Request::Asset(input), Vec::new())?;
        ensure!(!cancel.is_cancelled(), "File operation cancelled");
        match reply {
            Reply::Asset(mut asset)
                if asset.bytes.is_empty() && asset.size == bytes.len() as u64 =>
            {
                asset.bytes = bytes;
                Ok(asset)
            }
            Reply::Error(error) => Err(error.into()),
            _ => bail!("Unexpected WSL asset response"),
        }
    }

    pub(super) fn retire(&self) {
        self.retired.store(true, Ordering::Release);
        self.close();
    }

    pub(super) fn close(&self) {
        if let Ok(mut open) = self.open.try_lock() {
            open.take();
        }
    }
}

fn missing(error: &anyhow::Error) -> bool {
    file_error(error, ErrorCode::NotFound)
}

fn file_error(error: &anyhow::Error, code: ErrorCode) -> bool {
    error
        .downcast_ref::<FileError>()
        .is_some_and(|error| error.code == code)
}
