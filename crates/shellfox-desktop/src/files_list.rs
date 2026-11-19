use super::{DocumentBackend, terminal_links::SourcePosition};
use crate::root::Root;
use gpui::Context;
use shellfox_files::FileList;

#[derive(Clone, Copy)]
pub(crate) enum Navigation {
    List,
    Input,
    Create,
    Terminal(u64),
    TerminalLink(u64, Option<SourcePosition>),
}

pub(super) enum Target {
    Directory(FileList),
    File { parent: FileList, path: String },
    Missing(String),
}

impl Root {
    pub(crate) fn list_files(&mut self, path: String, cx: &mut Context<Self>) {
        self.navigate_files(path, Navigation::List, cx);
    }

    pub(crate) fn confirm_missing_file_folder(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .files
            .as_ref()
            .and_then(|files| files.missing_folder.clone())
        else {
            return;
        };
        self.navigate_files(path, Navigation::Create, cx);
    }

    pub(crate) fn navigate_files(
        &mut self,
        path: String,
        navigation: Navigation,
        cx: &mut Context<Self>,
    ) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files else {
            return;
        };
        if files.busy {
            return;
        }
        files.request += 1;
        let request = files.request;
        files.busy = true;
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        files.listing_error = None;
        files.missing_folder = None;
        files.completion.state.dismiss();
        let show_requested_path = backend.is_wsl()
            && (files.location.read(cx).text.is_empty()
                || matches!(navigation, Navigation::Terminal(_) | Navigation::TerminalLink(..)));
        if show_requested_path {
            files.completion.seen = (path.clone(), false);
        }
        files.location.update(cx, |field, cx| {
            if show_requested_path {
                field.show_file_location(path.clone(), cx);
            }
            field.set_read_only(true, cx);
        });
        let target = path.clone();
        let task = cx.background_executor().spawn(async move {
            match navigation {
                Navigation::Input | Navigation::TerminalLink(..) => {
                    task_backend.lookup_path(&target).await
                }
                Navigation::Create => task_backend
                    .create_directory(&target)
                    .await
                    .map(Target::Directory),
                _ => task_backend.list(&target).await.map(Target::Directory),
            }
        });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let current = root.files_result_current(epoch, &backend);
                let terminal = match navigation {
                    Navigation::Terminal(epoch) | Navigation::TerminalLink(epoch, _) => Some(epoch),
                    _ => None,
                };
                let obsolete =
                    terminal.is_some_and(|epoch| epoch != root.navigation.epoch() || root.settings);
                let Some(files) = &mut root.files else {
                    return;
                };
                if files.request != request || !files.backend.same_origin(&backend) {
                    return;
                }
                files.busy = false;
                files
                    .location
                    .update(cx, |field, cx| field.set_read_only(false, cx));
                if !current || obsolete {
                    if !current {
                        root.error = Some("Folder result belongs to a retired source.".into());
                    }
                    cx.notify();
                    return;
                }
                let file = match result {
                    Ok(Target::Directory(listing)) => {
                        if publish_listing(files, listing, &backend, cx) && terminal.is_some() {
                            files.visible = true;
                            files.browser = true;
                            files.browser_before_maximize = true;
                        }
                        None
                    }
                    Ok(Target::File { parent, path }) => {
                        publish_listing(files, parent, &backend, cx).then_some(path)
                    }
                    Ok(Target::Missing(path)) => {
                        if matches!(navigation, Navigation::TerminalLink(..)) {
                            files.listing_error = Some((path, "File not found.".into()));
                        } else {
                            files.missing_folder = Some(path);
                        }
                        None
                    }
                    Err(error) => {
                        files.listing_error = Some((path, error.to_string()));
                        None
                    }
                };
                if let Some(path) = file {
                    match navigation {
                        Navigation::TerminalLink(epoch, Some(position)) => root
                            .request_file_action(
                                super::pending::FileAction::OpenAt(path, epoch, position),
                                cx,
                            ),
                        _ => root.request_open_file(path, cx),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn publish_listing(
    files: &mut super::Files,
    listing: FileList,
    backend: &DocumentBackend,
    cx: &mut Context<Root>,
) -> bool {
    let label = listing.path.clone();
    let mut prepared = Some(listing);
    if let Err(error) =
        backend.commit_if_current(|| std::mem::swap(&mut files.listing, &mut prepared))
    {
        files.listing_error = Some((label, format!("Folder source retired: {error:#}")));
        return false;
    }
    files
        .tree
        .reset(prepared.as_ref().is_some_and(|old| old.path == label));
    files.completion.seen = (label.clone(), false);
    files
        .location
        .update(cx, |field, cx| field.show_file_location(label, cx));
    true
}
