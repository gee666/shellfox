use super::{
    DocumentBackend,
    terminal_links::{LinkOrigin, SourcePosition},
};
use crate::root::Root;
use gpui::Context;
use shellfox_files::FileList;

#[derive(Clone, Copy)]
pub(crate) enum Navigation {
    List,
    Input,
    Create,
    Terminal(LinkOrigin),
    TerminalLink(LinkOrigin, Option<SourcePosition>),
}

pub(crate) struct ListingError {
    pub message: String,
    pub retry: Option<String>,
}

impl ListingError {
    pub(super) fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retry: None,
        }
    }
}

/// A terminal link that named no file or folder, shown in the document area.
pub(crate) struct LinkError {
    /// The absolute location that was searched, for the copy button.
    pub path: String,
    pub origin: LinkOrigin,
    pub position: Option<SourcePosition>,
}

pub(crate) enum ListingView<'a> {
    Loading,
    Retry {
        message: &'a str,
        path: &'a str,
    },
    Rows {
        listing: Option<&'a FileList>,
        message: Option<&'a str>,
    },
}

impl<'a> ListingView<'a> {
    pub fn new(
        loading: bool,
        listing: Option<&'a FileList>,
        error: Option<&'a ListingError>,
    ) -> Self {
        if loading {
            return Self::Loading;
        }
        if let Some(error) = error
            && let Some(path) = error.retry.as_deref()
        {
            return Self::Retry {
                message: &error.message,
                path,
            };
        }
        Self::Rows {
            listing,
            message: error.map(|error| error.message.as_str()),
        }
    }
}

pub(super) fn initial_folder(preferred: Option<String>, scoped: bool) -> String {
    preferred.unwrap_or_else(|| if scoped { "." } else { "~" }.into())
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
            .panel
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
        let Some(files) = &mut self.files.panel else {
            return;
        };
        if files.busy() && !(files.tree.loading && matches!(navigation, Navigation::Terminal(_))) {
            return;
        }
        let request = files.activity.begin();
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        files.listing_error = None;
        files.missing_folder = None;
        if matches!(navigation, Navigation::TerminalLink(..)) {
            files.link_error = None;
        }
        files.completion.state.dismiss();
        if matches!(navigation, Navigation::List | Navigation::Terminal(_)) {
            files.tree.begin_reload();
        }
        let show_requested_path = files.location.read(cx).text.is_empty()
            || (backend.is_wsl()
                && matches!(
                    navigation,
                    Navigation::Terminal(_) | Navigation::TerminalLink(..)
                ));
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
                    Navigation::Terminal(origin) | Navigation::TerminalLink(origin, _) => {
                        Some(origin)
                    }
                    _ => None,
                };
                let obsolete = terminal.is_some_and(|origin| !root.link_is_current(origin));
                let Some(files) = &mut root.files.panel else {
                    return;
                };
                if !files.backend.same_origin(&backend) || !files.activity.finish(request) {
                    return;
                }
                files.tree.loading = false;
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
                        if let Navigation::TerminalLink(origin, position) = navigation {
                            files.link_error = Some(LinkError {
                                path,
                                origin,
                                position,
                            });
                        } else {
                            files.missing_folder = Some(path);
                        }
                        None
                    }
                    Err(error) => {
                        files.listing_error = Some(ListingError {
                            message: error.to_string(),
                            retry: matches!(navigation, Navigation::List).then_some(path),
                        });
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
        files.listing_error = Some(ListingError::message(format!(
            "Folder source retired: {error:#}"
        )));
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

#[cfg(test)]
mod folder_race_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_hides_stale_rows_through_loading_and_retry_then_shows_the_new_listing() {
        let old = FileList {
            path: "/folder".into(),
            parent_path: Some("/".into()),
            entries: Vec::new(),
            truncated: false,
        };
        let failure = ListingError {
            message: "Folder unavailable".into(),
            retry: Some(old.path.clone()),
        };
        assert!(matches!(
            ListingView::new(true, Some(&old), None),
            ListingView::Loading
        ));
        assert!(matches!(
            ListingView::new(false, Some(&old), Some(&failure)),
            ListingView::Retry {
                message: "Folder unavailable",
                path: "/folder"
            }
        ));
        assert!(matches!(
            ListingView::new(true, Some(&old), Some(&failure)),
            ListingView::Loading
        ));
        let fresh = FileList {
            truncated: true,
            ..old
        };
        assert!(matches!(
            ListingView::new(false, Some(&fresh), None),
            ListingView::Rows { listing: Some(listing), message: None } if listing.truncated
        ));
    }

    #[test]
    fn initial_folder_prefers_the_start_directory_and_defaults_to_its_scope() {
        assert_eq!(initial_folder(Some("/work".into()), false), "/work");
        assert_eq!(initial_folder(Some("/work".into()), true), "/work");
        assert_eq!(initial_folder(None, true), ".");
        assert_eq!(initial_folder(None, false), "~");
    }
}
