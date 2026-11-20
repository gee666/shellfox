use super::{Files, list::ListingError};
use crate::{directory::completion::Completion, field::Field, root::Root, theme::Palette};
use gpui::{
    App, Context, Entity, FocusHandle, ScrollHandle, Subscription, Window, div, prelude::*, px,
};
use std::{cell::RefCell, time::Duration};

pub(crate) struct BrowserFocus {
    pub region: FocusHandle,
    pub toggle: FocusHandle,
    pub line: FocusHandle,
    pub browse: FocusHandle,
}

impl BrowserFocus {
    pub fn new(cx: &App) -> Self {
        Self {
            region: cx.focus_handle(),
            toggle: cx.focus_handle(),
            line: cx.focus_handle(),
            browse: cx.focus_handle(),
        }
    }
}

#[derive(Default)]
pub(crate) struct FileCompletion {
    pub state: Completion,
    pub scroll: ScrollHandle,
    pub seen: (String, bool),
    focus_subscriptions: RefCell<Option<[Subscription; 2]>>,
}

impl FileCompletion {
    pub(crate) fn track_focus(
        &self,
        field: &Entity<Field>,
        window: &mut Window,
        cx: &mut Context<Root>,
    ) {
        let mut subscriptions = self.focus_subscriptions.borrow_mut();
        if subscriptions.is_some() {
            return;
        }
        let focus = field.read(cx).edit_focus();
        let id = field.entity_id();
        let on_focus = cx.on_focus(&focus, window, move |root, _, cx| {
            if let Some(files) = root
                .files
                .as_mut()
                .filter(|files| files.location.entity_id() == id)
            {
                files.completion.state.reset();
                root.query_file_matches(false, cx);
            }
        });
        let on_blur = cx.on_blur(&focus, window, move |root, _, cx| {
            if let Some(files) = root
                .files
                .as_mut()
                .filter(|files| files.location.entity_id() == id)
            {
                files.completion.state.dismiss();
                cx.notify();
            }
        });
        *subscriptions = Some([on_focus, on_blur]);
    }
}

impl Root {
    pub(super) fn file_location_changed(&mut self, field: Entity<Field>, cx: &mut Context<Self>) {
        let Some(files) = &mut self.files else {
            return;
        };
        if files.location != field {
            return;
        }
        let field = field.read(cx);
        let input = (field.text.clone(), field.is_composing());
        if input == files.completion.seen {
            return;
        }
        files.completion.seen = input;
        files.completion.state.reset();
        if files.busy {
            return;
        }
        if files
            .listing_error
            .as_ref()
            .is_some_and(|error| error.retry.is_none())
        {
            files.listing_error = None;
        }
        files.missing_folder = None;
        self.query_file_matches(false, cx);
    }

    pub(crate) fn reopen_file_completion(&mut self, cx: &mut Context<Self>) {
        let Some(files) = &mut self.files else { return };
        if files.completion.state.reopen() {
            files.completion.state.reset();
            self.query_file_matches(false, cx);
        }
    }

    pub(crate) fn file_completion_key(
        &mut self,
        key: &str,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(files) = &mut self.files else {
            return false;
        };
        if files.busy
            || files.missing_folder.is_some()
            || !files.location.read(cx).is_focused(window)
        {
            return false;
        }
        let input = &files.location.read(cx).text;
        match key {
            "escape" => files.completion.state.dismiss(),
            "up" | "down" if files.completion.state.move_active(input, key == "up") => {
                files
                    .completion
                    .scroll
                    .scroll_to_item(files.completion.state.active());
            }
            "tab" if shift && files.backend.is_local() => {
                files.completion.state.dismiss();
                window.focus(&files.browser_focus.browse);
            }
            "tab" if !shift => {
                if files.completion.state.dismissed {
                    files.completion.state.reset();
                }
                if let Some(value) = files.completion.state.tab() {
                    accept(files, value, cx);
                } else {
                    self.query_file_matches(true, cx);
                }
            }
            "enter" => {
                if let Some(value) = files.completion.state.choose(input, None) {
                    accept(files, value, cx);
                } else {
                    return false;
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    pub(crate) fn choose_file_completion(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(files) = self.files.as_mut().filter(|files| !files.busy) else {
            return;
        };
        let input = &files.location.read(cx).text;
        if let Some(value) = files.completion.state.choose(input, Some(index)) {
            accept(files, value, cx);
            files.location.read(cx).focus(window);
            cx.notify();
        }
    }

    fn query_file_matches(&mut self, cycle: bool, cx: &mut Context<Self>) {
        if !self.files_source_current() {
            return;
        }
        let Some(files) = &mut self.files else {
            return;
        };
        if !files.visible
            || !files.browser
            || files.busy
            || files.missing_folder.is_some()
            || files.location.read(cx).is_composing()
        {
            return;
        }
        let path = files.location.read(cx).text.clone();
        files.completion.state.cancel();
        let ticket = files.completion.state.ticket;
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let worker = backend.clone();
        let target = path.clone();
        let timer = cx.background_executor().timer(Duration::from_millis(120));
        cx.spawn(async move |root, cx| {
            if !cycle {
                timer.await;
            }
            // A rejected partial prefix must never be sent to the peer after a later edit or Go.
            let matches_current = |root: &Root, cx: &gpui::App| {
                root.files_result_current(epoch, &backend)
                    && root.files.as_ref().is_some_and(|files| {
                        files.visible
                            && files.browser
                            && !files.busy
                            && files.completion.state.ticket == ticket
                            && files.location.read(cx).text == path
                            && !files.location.read(cx).is_composing()
                    })
            };
            if !root
                .update(cx, |root, cx| matches_current(root, cx))
                .unwrap_or(false)
            {
                return;
            }
            let result = cx
                .background_executor()
                .spawn(async move { worker.complete(&target).await })
                .await;
            let _ = root.update(cx, |root, cx| {
                if !matches_current(root, cx) {
                    return;
                }
                let Some(files) = &mut root.files else {
                    return;
                };
                match result {
                    Ok(matches) => {
                        files.completion.state.arrive(matches);
                        if cycle {
                            if let Some(value) = files.completion.state.tab() {
                                accept(files, value, cx);
                            } else {
                                files.listing_error =
                                    Some(ListingError::message("No matching folders"));
                            }
                        }
                    }
                    Err(error) if cycle => {
                        files.listing_error = Some(ListingError::message(error.to_string()));
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn accept(files: &mut Files, value: String, cx: &mut Context<Root>) {
    files.completion.seen = (value.clone(), false);
    files
        .completion
        .scroll
        .scroll_to_item(files.completion.state.active());
    files
        .location
        .update(cx, |field, cx| field.set_text(value, cx));
}

pub(crate) fn completion_list(
    matches: &[String],
    files: &Files,
    palette: Palette,
    window: &Window,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let active = files.completion.state.active();
    let max_height = (f32::from(window.viewport_size().height) * 0.35).min(240.);
    div()
        .id("file-directory-completions")
        .absolute()
        .top(px(35.))
        .left(px(4.))
        .right(px(4.))
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
            window.prevent_default()
        })
        .track_scroll(&files.completion.scroll)
        .overflow_y_scroll()
        .max_h(px(max_height))
        .p(px(2.))
        .border_1()
        .border_color(palette.input_border)
        .rounded(px(2.))
        .bg(palette.input_bg)
        .font_family("Cascadia Code")
        .text_size(px(12.))
        .line_height(px(20.))
        .children(matches.iter().enumerate().map(|(index, value)| {
            div()
                .id(("file-match", index))
                .flex()
                .items_center()
                .h(px(26.))
                .gap(px(6.))
                .px(px(6.))
                .border_1()
                .border_color(gpui::transparent_black())
                .when(index == active, |style| {
                    style
                        .bg(palette.accent.opacity(0.12))
                        .border_color(palette.accent)
                })
                .hover(|style| style.bg(palette.hover))
                .cursor_pointer()
                .child(
                    crate::assets::icon("folder")
                        .size(px(16.))
                        .flex_shrink_0()
                        .text_color(palette.muted),
                )
                .child(div().min_w_0().truncate().child(value.clone()))
                .on_click(cx.listener(move |root, _, window, cx| {
                    root.choose_file_completion(index, window, cx)
                }))
        }))
}
