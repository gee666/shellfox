use crate::workspace::AfterAction;
use crate::{
    field::Field,
    root::{Dialog, Root},
};
use gpui::{Context, Entity, KeyDownEvent, Window};
use shellfox_files::{Access, EntryKind, FileService, Location};
use shellfox_host::{CreateSession, HostRequest, ProfileId, SessionId};
use shellfox_service::native::NativeAction;
use std::path::{Path, PathBuf};

pub struct Draft {
    pub path: Entity<Field>,
    pub matches: Vec<String>,
    pub active: usize,
    pub request_id: SessionId,
    pub last_query: Option<String>,
    pub accepted: Option<String>,
    pub busy: bool,
    pub error: Option<String>,
}
impl Root {
    pub(crate) fn create_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(feature="experimental-shellfox-native")]
        if !self.is_local_source(){self.focus_peer_create(window,cx);return;}
        if self.user_action_pending || self.closing_window {
            return;
        }
        let path = Field::directory(cx);
        path.read(cx).focus(window);
        cx.observe(&path, |root, _, cx| root.refresh_directory_matches(cx))
            .detach();
        self.dialog = Some(Dialog::Create(Draft {
            path,
            matches: Vec::new(),
            active: 0,
            request_id: SessionId::new(),
            last_query: None,
            accepted: None,
            busy: false,
            error: None,
        }));
        self.settings = false;
        self.session_menu = None;
        self.refresh_directory_matches(cx);
        cx.notify();
    }
    pub(crate) fn refresh_directory_matches(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return;
        };
        let query = draft.path.read(cx).text.clone();
        if draft.busy
            || draft.last_query.as_ref() == Some(&query)
            || draft.path.read(cx).is_composing()
        {
            return;
        }
        if draft.accepted.as_ref() == Some(&query) {
            draft.matches.clear();
            return;
        }
        draft.accepted = None;
        draft.last_query = Some(query.clone());
        draft.error = None;
        let request_id = draft.request_id;
        let (directory, needle, prefix) = completion_query(&self.run_dir, &query);
        let task = cx.background_executor().spawn(async move {
            FileService::local_desktop(Access::ReadOnly)
                .list(&Location {
                    path: directory.to_string_lossy().into_owned(),
                })
                .map(|listing| {
                    listing
                        .entries
                        .into_iter()
                        .filter(|entry| {
                            entry.kind == EntryKind::Directory
                                && entry
                                    .name
                                    .to_lowercase()
                                    .starts_with(&needle.to_lowercase())
                        })
                        .take(100)
                        .map(|entry| format!("{prefix}{}{}", entry.name, std::path::MAIN_SEPARATOR))
                        .collect::<Vec<_>>()
                })
        });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let Some(Dialog::Create(draft)) = &mut root.dialog else {
                    return;
                };
                if draft.request_id != request_id || draft.path.read(cx).text != query {
                    return;
                }
                draft.matches = result.unwrap_or_default();
                draft.active = 0;
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn directory_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return;
        };
        if draft.busy || self.user_action_pending || draft.path.read(cx).is_composing() {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                if draft.matches.is_empty() {
                    self.dialog = None;
                } else {
                    draft.matches.clear();
                }
            }
            "up" | "down" if !draft.matches.is_empty() => {
                let count = draft.matches.len();
                draft.active = if event.keystroke.key == "up" {
                    (draft.active + count - 1) % count
                } else {
                    (draft.active + 1) % count
                };
            }
            "tab" if !event.keystroke.modifiers.shift => {
                self.choose_directory_match(None, window, cx);
            }
            "enter" => {
                if draft.matches.is_empty() {
                    self.submit_directory(window, cx);
                } else {
                    self.choose_directory_match(None, window, cx);
                }
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }
    pub(crate) fn choose_directory_match(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(Dialog::Create(draft)) = &mut self.dialog
            && let Some(value) = draft.matches.get(index.unwrap_or(draft.active)).cloned()
        {
            draft.accepted = Some(value.clone());
            draft.path.update(cx, |field, cx| field.set_text(value, cx));
            draft.path.read(cx).focus(window);
            draft.matches.clear();
        }
        cx.notify();
    }
    pub(crate) fn submit_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return;
        };
        if draft.busy || draft.path.read(cx).is_composing() {
            return;
        }
        let query = draft.path.read(cx).text.clone();
        let candidate = resolve_path(&self.run_dir, &query);
        let request_id = draft.request_id;
        draft.busy = true;
        draft.matches.clear();
        draft.error = None;
        let task = cx.background_executor().spawn(async move {
            let service = FileService::local_desktop(Access::ReadOnly);
            let location = Location {
                path: candidate.to_string_lossy().into_owned(),
            };
            service.stat(&location).and_then(|stat| {
                if stat.kind != EntryKind::Directory {
                    return Err(shellfox_files::FileError {
                        code: shellfox_files::ErrorCode::Validation,
                        message: "Choose a directory.".into(),
                    });
                }
                Ok(location.path)
            })
        });
        cx.spawn_in(window, async move |root, cx| {
            let result = task.await;
            let _ = root.update_in(cx, |root, window, cx| {
                let Some(Dialog::Create(draft)) = &mut root.dialog else {
                    return;
                };
                if draft.request_id != request_id {
                    return;
                }
                draft.busy = false;
                if draft.path.read(cx).text != query {
                    draft.error = Some("Folder changed while checking. Submit it again.".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok(cwd) => {
                        let profile = ProfileId::new(root.default_profile());
                        match profile {
                            Ok(profile_id) => root.submit_native(
                                NativeAction::Metadata(HostRequest::CreateSession(CreateSession {
                                    request_id,
                                    title: None,
                                    cwd,
                                    profile_id,
                                })),
                                AfterAction::Create,
                                window,
                                cx,
                            ),
                            Err(error) => root.error = Some(error.to_string()),
                        }
                    }
                    Err(error) => {
                        if let Some(Dialog::Create(draft)) = &mut root.dialog {
                            draft.error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn browse_directory(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(Dialog::Create(draft)) = &self.dialog else {
            return;
        };
        let request_id = draft.request_id;
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select folder".into()),
        });
        cx.spawn_in(window, async move |root, cx| {
            let result = prompt.await;
            let _ = root.update_in(cx, |root, window, cx| {
                let Some(Dialog::Create(draft)) = &mut root.dialog else {
                    return;
                };
                if draft.request_id != request_id {
                    return;
                }
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            draft.path.update(cx, |field, cx| {
                                field.set_text(path.to_string_lossy().into_owned(), cx)
                            });
                            root.submit_directory(window, cx);
                        }
                    }
                    Ok(Ok(None)) => draft.path.read(cx).focus(window),
                    _ => draft.error = Some("Folder picker failed.".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
fn resolve_path(home: &Path, value: &str) -> PathBuf {
    if value == "~" || value.is_empty() {
        home.to_owned()
    } else if value.starts_with("~/") || value.starts_with("~\\") {
        home.join(&value[2..])
    } else if Path::new(value).is_absolute() {
        value.into()
    } else {
        home.join(value)
    }
}
fn completion_query(home: &Path, value: &str) -> (PathBuf, String, String) {
    if value.ends_with(['\\', '/']) {
        return (resolve_path(home, value), String::new(), value.to_owned());
    }
    let (prefix, needle) = value.rfind(['\\', '/']).map_or(("", value), |index| {
        (&value[..index + 1], &value[index + 1..])
    });
    (
        resolve_path(home, prefix),
        needle.to_owned(),
        prefix.to_owned(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_retains_typed_relative_prefix() {
        let home = Path::new("/private/run");
        let (path, needle, prefix) = completion_query(home, "project/sub");
        assert_eq!(path, home.join("project/"));
        assert_eq!(needle, "sub");
        assert_eq!(prefix, "project/");
    }
}
