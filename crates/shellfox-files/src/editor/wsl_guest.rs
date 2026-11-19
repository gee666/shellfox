use super::*;
use crate::wsl_files_ipc::{self as wire, Reply, Request};
use std::{fs, io, os::unix::fs::DirBuilderExt, path::PathBuf};
#[path = "wsl_guest_transfer.rs"]
mod transfer;
use crate::{TransferActor, TransferReply, TransferRequest};

pub fn serve_wsl_files(mut input: impl io::Read, mut output: impl io::Write) -> io::Result<()> {
    // SAFETY: these Linux calls take no pointers and have no side effects.
    let (uid, euid, gid, egid) = unsafe {
        (
            libc::getuid(),
            libc::geteuid(),
            libc::getgid(),
            libc::getegid(),
        )
    };
    if uid == 0 || uid != euid || gid != egid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "WSL Files needs an ordinary guest user",
        ));
    }
    let mut editor = GuestEditor {
        service: Arc::new(FileService::local_desktop(Access::ReadWrite)),
        opened: None,
        journal: None,
        transfer_actor: TransferActor::new("verified-wsl-connection".into(), Uuid::new_v4())
            .map_err(io::Error::other)?,
    };
    wire::write(
        &mut output,
        &Reply::Ready {
            version: wire::VERSION,
            uid,
        },
        &[],
    )?;
    while let Some((request, bytes)) = wire::read(&mut input)? {
        let (reply, data) = editor
            .handle(request, bytes)
            .unwrap_or_else(|error| (Reply::Error(error), Vec::new()));
        wire::write(&mut output, &reply, &data)?;
    }
    Ok(())
}

struct GuestEditor {
    service: Arc<FileService>,
    opened: Option<HeldEditor>,
    journal: Option<GuestJournal>,
    transfer_actor: TransferActor,
}

impl GuestEditor {
    fn handle(&mut self, request: Request, bytes: Vec<u8>) -> Result<(Reply, Vec<u8>)> {
        if !bytes.is_empty() && !matches!(request, Request::Save(_) | Request::SaveImage(_)
            | Request::Transfer(TransferRequest::Write(_)) | Request::WriteInline(_) | Request::WriteImageInline(_)) {
            return Err(bad_payload());
        }
        let reply = match request {
            Request::List(location) => Reply::List(self.list(&location)?),
            Request::Open(location) => {
                self.opened = None;
                let (editor, document) = HeldEditor::open(&self.service, &location)?;
                self.opened = Some(editor);
                return Ok(document_packet(document));
            }
            Request::Save(mut input) => {
                if !input.text.is_empty() || bytes.len() as u64 > TRANSFER_TEXT_BYTES {
                    return Err(bad_payload());
                }
                input.text = String::from_utf8(bytes).map_err(|_| bad_payload())?;
                let (editor, backend) = self.save_target()?;
                Reply::Saved(editor.write(backend, &input)?)
            }
            Request::SaveImage(mut input) => {
                if !input.bytes.is_empty() {
                    return Err(bad_payload());
                }
                input.bytes = bytes;
                let (editor, backend) = self.save_target()?;
                Reply::Saved(editor.write_image(backend, &input)?)
            }
            Request::Asset(mut input) => {
                input.path = self
                    .service
                    .browser_existing(&Location { path: input.path })?
                    .path;
                self.ensure_journal()?;
                let assets = self
                    .journal
                    .as_ref()
                    .ok_or_else(unavailable)?
                    .backend
                    .session();
                let mut asset = assets.read_asset(&input, EditorCancellation::default())?;
                let data = std::mem::take(&mut asset.bytes);
                return Ok((Reply::Asset(asset), data));
            }
            Request::CreateDirectory(location) => {
                let location = self.service.browser_create_target(&location)?;
                self.service
                    .create_directory_with_context(&location, None)?;
                Reply::List(self.list(&location)?)
            }
            Request::Stat(location) => Reply::Stat(
                self.service
                    .stat(&self.service.browser_existing(&location)?)?,
            ),
            Request::Transfer(request) => return self.transfer(request, bytes),
            Request::ReadInline(location) => {
                let location = self.service.browser_existing(&location)?;
                return Ok(document_packet(self.service.read_file(&location)?));
            }
            Request::WriteInline(mut input) => {
                if !input.text.is_empty() || bytes.len() as u64 > TRANSFER_TEXT_BYTES { return Err(bad_payload()); }
                input.text = String::from_utf8(bytes).map_err(|_| bad_payload())?;
                Reply::Saved(self.service.write_text(&input)?)
            }
            Request::AssetInline(mut input) => {
                input.path = self.service.browser_existing(&Location { path: input.path })?.path;
                let mut asset = self.service.read_asset(&input)?;
                let bytes = std::mem::take(&mut asset.bytes);
                return Ok((Reply::Asset(asset), bytes));
            }
            Request::WriteImageInline(mut input) => {
                if !input.bytes.is_empty() { return Err(bad_payload()); }
                input.bytes = bytes;
                Reply::Saved(self.service.write_image(&input)?)
            }
        };
        Ok((reply, Vec::new()))
    }

    fn list(&self, location: &Location) -> Result<FileList> {
        self.service.list(&self.service.browser_existing(location)?)
    }

    fn save_target(&mut self) -> Result<(&mut HeldEditor, &EditorBackend)> {
        if self.opened.is_none() {
            return Err(FileError::conflict());
        }
        self.ensure_journal()?;
        let editor = self.opened.as_mut().ok_or_else(FileError::conflict)?;
        let backend = self.journal.as_ref().ok_or_else(unavailable)?;
        Ok((editor, &backend.backend))
    }

    fn ensure_journal(&mut self) -> Result<()> {
        if self.journal.is_none() {
            self.journal = Some(GuestJournal::new(self.service.clone())?);
        }
        Ok(())
    }
}

impl Drop for GuestEditor {
    fn drop(&mut self) {
        if let Some(journal) = &mut self.journal {
            if let Ok(mut registry) = journal.backend.registry.lock() {
                registry.cancel_actor(self.transfer_actor.owner());
            }
        }
    }
}

fn document_packet(mut document: ReadResult) -> (Reply, Vec<u8>) {
    let bytes = match &mut document {
        ReadResult::Text { text, .. } => std::mem::take(text).into_bytes(),
        ReadResult::Image { bytes, .. } => std::mem::take(bytes),
        ReadResult::Binary { .. } => Vec::new(),
    };
    (Reply::Document(document), bytes)
}

fn bad_payload() -> FileError {
    FileError::new(ErrorCode::Validation, "Invalid WSL Files payload.")
}

struct GuestJournal {
    root: PathBuf,
    backend: EditorBackend,
}

impl GuestJournal {
    fn new(service: Arc<FileService>) -> Result<Self> {
        let root = PathBuf::from("/tmp").join(format!("shellfox-wsl-files-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        // The journal policy requires a private parent, not the shared /tmp directory.
        let backend = match EditorBackend::new(service, root.join("journal")) {
            Ok(backend) => backend,
            Err(error) => {
                let _ = fs::remove_dir(&root);
                return Err(error);
            }
        };
        Ok(Self { root, backend })
    }
}

impl Drop for GuestJournal {
    fn drop(&mut self) {
        let clean = self.backend.registry.lock().ok().and_then(|registry| {
            let journal = registry.journal.as_ref()?.lock().ok()?;
            Some(
                journal.entries.is_empty()
                    && journal.live.is_empty()
                    && journal.cleanup_error.is_none()
                    && journal.metadata_stage.is_none(),
            )
        });
        // A failed or uncertain publication keeps its recovery evidence.
        if clean == Some(true) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
