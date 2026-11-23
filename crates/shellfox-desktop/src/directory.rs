use crate::{
    field::Field,
    root::{Dialog, Root},
};
use gpui::{Context, Entity, FocusHandle, ScrollHandle, Subscription, Window};
use shellfox_host::SessionId;
use std::{path::PathBuf, time::Duration};

pub(crate) mod completion;
mod keys;
mod paths;
mod peer;
mod submit;

pub(crate) fn validate_session_folder(cwd: &str) -> Result<PathBuf, String> {
    paths::resolve(std::path::Path::new(cwd), cwd, false)
        .map(PathBuf::from)
        .map_err(|_| "The session folder is missing or inaccessible.".into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Busy {
    Checking,
    Creating,
}

pub struct Draft {
    pub path: Entity<Field>,
    target: Option<peer::PeerDirectory>,
    pub home: Option<PathBuf>,
    pub scroll: ScrollHandle,
    pub scrollbar: crate::files::scrollbar::Scrollbar,
    pub browse: FocusHandle,
    pub close: FocusHandle,
    prior: Option<FocusHandle>,
    pub keep_editing: FocusHandle,
    pub create_missing: FocusHandle,
    id: SessionId,
    seen: (String, bool),
    completion: completion::Completion,
    pub busy: Option<Busy>,
    pub missing: Option<String>,
    pub error: Option<String>,
    _focus: [Subscription; 3],
}

impl Draft {
    pub(crate) fn matches(&self, cx: &gpui::App) -> &[String] {
        self.completion.visible(&self.path.read(cx).text)
    }

    pub(crate) fn peer_name(&self) -> Option<&str> {
        self.target.as_ref().map(|target| target.name.as_str())
    }

    pub(crate) fn active(&self) -> usize {
        self.completion.active()
    }
}

impl Root {
    pub(crate) fn can_create_local_session(&self) -> bool {
        self.host.is_some()
            && self.snapshot.is_some()
            && self.profiles.iter().any(|profile| {
                profile.available && (cfg!(windows) || profile.id == self.default_profile())
            })
    }

    pub(crate) fn open_local_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some()
            || self.user_action_pending
            || self.closing_window
            || !self.can_create_local_session()
        {
            return;
        }
        if !self.is_local_source() {
            self.select_local_source(window, cx);
        }
        self.show_directory(None, paths::home(), window, cx);
    }

    fn show_directory(
        &mut self,
        target: Option<peer::PeerDirectory>,
        home: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() || self.user_action_pending || self.closing_window {
            return;
        }
        let path = Field::directory(cx);
        let focus = path.read(cx).edit_focus();
        let observe = cx.observe(&path, |root, _, cx| root.directory_edit(cx));
        let blur = cx.on_blur(&focus, window, |root, _, cx| {
            if let Some(Dialog::Create(draft)) = &mut root.dialog {
                draft.completion.dismiss();
                cx.notify();
            }
        });
        let focused = cx.on_focus(&focus, window, |root, _, cx| {
            root.reopen_directory_matches(cx)
        });
        let error = (home.is_none() && target.is_none())
            .then(|| "Could not determine the home directory.".into());
        self.dialog = Some(Dialog::Create(Draft {
            path,
            target,
            home,
            scroll: ScrollHandle::new(),
            scrollbar: Default::default(),
            browse: cx.focus_handle(),
            close: cx.focus_handle(),
            prior: window.focused(cx),
            keep_editing: cx.focus_handle(),
            create_missing: cx.focus_handle(),
            id: SessionId::new(),
            seen: (String::new(), false),
            completion: Default::default(),
            busy: None,
            missing: None,
            error,
            _focus: [observe, blur, focused],
        }));
        self.hide_settings(cx);
        self.session_menu = None;
        focus.focus(window);
        // The input has not been painted yet, so its first focus event may not fire.
        self.query_directory_matches(false, cx);
        cx.notify();
    }

    fn directory_edit(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return;
        };
        let field = draft.path.read(cx);
        let seen = (field.text.clone(), field.is_composing());
        if draft.seen == seen {
            return;
        }
        keep_confirmation_for_path(&mut draft.missing, &seen.0);
        draft.seen = seen;
        draft.error = None;
        draft.completion.reset();
        self.query_directory_matches(false, cx);
        cx.notify();
    }

    pub(crate) fn reopen_directory_matches(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return;
        };
        if !draft.completion.reopen() {
            return;
        }
        self.query_directory_matches(false, cx);
        cx.notify();
    }

    fn query_directory_matches(&mut self, tab: bool, cx: &mut Context<Self>) {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return;
        };
        if draft.busy.is_some() || draft.missing.is_some() || draft.seen.1 {
            return;
        }
        let home = draft.home.clone();
        let target = draft.target.clone();
        if home.is_none() && target.is_none() {
            return;
        }
        let query = draft.seen.0.clone();
        draft.completion.cancel();
        let (id, ticket) = (draft.id, draft.completion.ticket);
        cx.spawn(async move |root, cx| {
            if !tab {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
            }
            let current = root
                .read_with(cx, |root, _| {
                    matches!(&root.dialog,
                Some(Dialog::Create(draft)) if draft.id == id && draft.completion.ticket == ticket)
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let result = if let Some(target) = target {
                target.complete(query).await
            } else if let Some(home) = home {
                cx.background_executor()
                    .spawn(async move { paths::complete(&home, &query) })
                    .await
            } else {
                return;
            };
            let _ = root.update(cx, |root, cx| {
                let Some(Dialog::Create(draft)) = &mut root.dialog else {
                    return;
                };
                if draft.id != id || draft.completion.ticket != ticket {
                    return;
                }
                match result {
                    Ok(matches) => {
                        draft.completion.arrive(matches);
                        if tab {
                            if let Some(value) = draft.completion.tab() {
                                Self::accept_directory_match(draft, value, cx);
                            } else {
                                draft.error = Some("No matching folders".into());
                            }
                        }
                    }
                    Err(error) if tab => draft.error = Some(error.message),
                    Err(_) => return,
                }
                draft
                    .scroll
                    .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
                cx.notify();
            });
        })
        .detach();
    }

    fn accept_directory_match(draft: &mut Draft, value: String, cx: &mut Context<Self>) {
        draft.seen = (value.clone(), false);
        draft.error = None;
        draft.path.update(cx, |field, cx| field.set_text(value, cx));
    }

    pub(crate) fn choose_directory_match(
        &mut self,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(Dialog::Create(draft)) = &mut self.dialog else {
            return false;
        };
        let Some(value) = draft.completion.choose(&draft.seen.0, index) else {
            return false;
        };
        Self::accept_directory_match(draft, value, cx);
        cx.notify();
        true
    }
}

fn keep_confirmation_for_path(missing: &mut Option<String>, input: &str) {
    if missing.as_deref().is_some_and(|path| path != input) {
        *missing = None;
    }
}

#[cfg(test)]
mod tests {
    use super::keep_confirmation_for_path;

    #[test]
    fn non_text_input_notifications_keep_the_missing_folder_confirmation() {
        let mut missing = Some("new/folder".into());
        keep_confirmation_for_path(&mut missing, "new/folder");
        assert_eq!(missing.as_deref(), Some("new/folder"));
        keep_confirmation_for_path(&mut missing, "new/another-folder");
        assert_eq!(missing, None);
    }
}
