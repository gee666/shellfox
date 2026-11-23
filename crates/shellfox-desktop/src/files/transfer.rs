use super::DocumentBackend;
use crate::{editor_composition::EditorOutcome, root::Root};
use anyhow::{Context as _, Result};
use gpui::{Context, PathPromptOptions, Window};
use shellfox_files::{EditorCancellation, EditorSession};
use std::sync::{Arc, Mutex};
mod io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferDirection {
    Upload,
    Download,
}

#[derive(Clone, Default)]
pub(crate) struct TransferProgress {
    pub name: String,
    pub total: u64,
    pub offset: u64,
}
pub(crate) struct Transfer {
    pub direction: TransferDirection,
    pub cancel: EditorCancellation,
    progress: Mutex<TransferProgress>,
    /// Bumped when the shown progress changes: start, and each whole percent.
    changes: tokio::sync::watch::Sender<()>,
}
impl Transfer {
    fn new(direction: TransferDirection) -> Self {
        Self {
            direction,
            cancel: EditorCancellation::default(),
            progress: Mutex::default(),
            changes: tokio::sync::watch::channel(()).0,
        }
    }
    pub fn progress(&self) -> TransferProgress {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn start(&self, name: String, total: u64) {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = TransferProgress {
            name,
            total,
            offset: 0,
        };
        self.changes.send_replace(());
    }
    fn advance(&self, offset: u64) {
        let mut progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        let percent = |offset: u64| (offset.min(progress.total) * 100).checked_div(progress.total);
        let moved = percent(progress.offset) != percent(offset);
        progress.offset = offset;
        drop(progress);
        if moved {
            self.changes.send_replace(());
        }
    }
}

struct Call {
    backend: DocumentBackend,
    local: Arc<EditorSession>,
    epoch: u64,
    folder: String,
    state: Arc<Transfer>,
}

impl Root {
    pub(crate) fn begin_file_transfer(
        &mut self,
        direction: TransferDirection,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files.panel else {
            return;
        };
        if files.backend.is_local() || files.transfer.is_some() {
            return;
        }
        let Some(folder) = files.listing.as_ref().map(|list| list.path.clone()) else {
            return;
        };
        let selected = files
            .opened_path()
            .or(files.document_path.as_deref())
            .map(str::to_owned);
        if direction == TransferDirection::Download && selected.is_none() {
            return;
        }
        let Some(EditorOutcome::Ready(local)) = cx.try_global::<EditorOutcome>() else {
            return;
        };
        let state = Arc::new(Transfer::new(direction));
        let call = Call {
            backend: files.backend.clone(),
            local: local.session(),
            epoch: files.source_epoch,
            folder,
            state: state.clone(),
        };
        files.transfer = Some(state.clone());
        match direction {
            TransferDirection::Upload => {
                let picker = crate::files::view::picker::open(window, cx, PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some("Upload file".into()),
                });
                cx.spawn_in(window, async move |root, cx| {
                    let result = async {
                        let Some(path) = picker.await? else { return Ok(None); };
                        call.backend.check_current()?;
                        let task = cx.background_executor().spawn(io::upload(
                            call.backend.clone(),
                            call.local.clone(),
                            call.folder.clone(),
                            path,
                            call.state.clone(),
                        ));
                        task.await.map(Some)
                    }
                    .await;
                    let _ = root.update_in(cx, |root, _, cx| {
                        root.finish_file_transfer(call, result, cx)
                    });
                })
                .detach();
            }
            TransferDirection::Download => {
                let Some(selected) = selected else {
                    return;
                };
                cx.spawn_in(window, async move |root, cx| {
                    let result = async {
                        let header = cx
                            .background_executor()
                            .spawn(io::download_header(
                                call.backend.clone(),
                                selected,
                                call.state.clone(),
                            ))
                            .await?;
                        let suggested = io::safe_name(
                            TransferDirection::Download,
                            super::pending::basename(&header.path),
                        );
                        let picker = root
                            .update_in(cx, |root, _, cx| {
                                if !root.files_result_current(call.epoch, &call.backend)
                                    || call.state.cancel.is_cancelled()
                                {
                                    return None;
                                }
                                let dir = std::env::home_dir()
                                    .map(|home| {
                                        let downloads = home.join("Downloads");
                                        if downloads.is_dir() { downloads } else { home }
                                    })
                                    .unwrap_or_else(|| root.run_dir.clone());
                                Some(cx.prompt_for_new_path(&dir, Some(&suggested)))
                            })?
                            .context("The original peer file is no longer selected.")?;
                        let Some(path) = picker.await?? else {
                            return Ok(None);
                        };
                        let task = cx.background_executor().spawn(io::download(
                            call.backend.clone(),
                            call.local.clone(),
                            header,
                            path,
                            call.state.clone(),
                        ));
                        task.await.map(Some)
                    }
                    .await;
                    let _ = root.update_in(cx, |root, _, cx| {
                        root.finish_file_transfer(call, result, cx)
                    });
                })
                .detach();
            }
        }
        // Repaint when the shown progress changes; ends with the transfer.
        let mut changes = state.changes.subscribe();
        let state = Arc::downgrade(&state);
        cx.spawn(async move |root, cx| {
            while changes.changed().await.is_ok() {
                if !root
                    .update(cx, |root, cx| {
                        let active = root
                            .files.panel
                            .as_ref()
                            .and_then(|files| files.transfer.as_ref())
                            .is_some_and(|transfer| std::ptr::eq(Arc::as_ptr(transfer), state.as_ptr()));
                        if active {
                            cx.notify();
                        }
                        active
                            || root.files.retained.iter().any(|files| {
                                files
                                    .transfer
                                    .as_ref()
                                    .is_some_and(|transfer| std::ptr::eq(Arc::as_ptr(transfer), state.as_ptr()))
                            })
                    })
                    .unwrap_or(false)
                {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn cancel_file_transfer(&mut self, cx: &mut Context<Self>) {
        if let Some(transfer) = self
            .files.panel
            .as_ref()
            .and_then(|files| files.transfer.as_ref())
        {
            transfer.cancel.cancel();
            cx.notify();
        }
    }

    fn finish_file_transfer(
        &mut self,
        call: Call,
        result: Result<Option<String>>,
        cx: &mut Context<Self>,
    ) {
        let current = self.files_source_current()
            && self
                .files.panel
                .as_ref()
                .is_some_and(|f| f.backend.same_origin(&call.backend));
        let Some(files) = self
            .files.panel
            .iter_mut()
            .chain(self.files.retained.iter_mut())
            .find(|files| {
                files
                    .transfer
                    .as_ref()
                    .is_some_and(|transfer| Arc::ptr_eq(transfer, &call.state))
            })
        else {
            return;
        };
        files.transfer = None;
        let upload = call.state.direction == TransferDirection::Upload;
        let refresh = current
            && upload
            && files
                .listing
                .as_ref()
                .is_some_and(|list| list.path == call.folder);
        match result {
            Ok(Some(name)) => {
                self.notify_info(
                    cx,
                    format!("{} {name}", if upload { "Uploaded" } else { "Downloaded" }),
                );
                if refresh {
                    self.list_files(call.folder, cx);
                }
            }
            _ if call.state.cancel.is_cancelled() => self.notify_info(cx, "Transfer cancelled"),
            Err(error) => self.error = Some(error.to_string()),
            Ok(None) => {}
        }
        cx.notify();
    }
}
