use crate::{directory::completion::Completion, field::Field, root::Root};
use gpui::{Context, Entity, ScrollHandle, Window};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct FileCompletion {
    pub state: Completion,
    pub scroll: ScrollHandle,
    pub seen: (String, bool),
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
        files.listing_error = None;
        files.missing_folder = None;
        self.query_file_matches(false, cx);
    }

    pub(crate) fn reopen_file_completion(&mut self, cx: &mut Context<Self>) {
        if let Some(files) = &mut self.files {
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
            "escape" if !files.completion.state.visible(input).is_empty() => {
                files.completion.state.dismiss()
            }
            "up" | "down" if files.completion.state.move_active(input, key == "up") => {
                files
                    .completion
                    .scroll
                    .scroll_to_item(files.completion.state.active());
            }
            "tab" if shift && files.backend.is_local() => {
                files.completion.state.dismiss();
                window.focus(&files.browse_focus);
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
                    self.submit_file_path(cx);
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
        if let Some(files) = &mut self.files
            && !files.busy
        {
            let input = &files.location.read(cx).text;
            if let Some(value) = files.completion.state.choose(input, Some(index)) {
                accept(files, value, cx);
                files.location.read(cx).focus(window);
                cx.notify();
            }
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
                                files.listing_error = Some((path.clone(), "No matching folders".into()));
                            }
                        }
                    }
                    Err(error) if cycle => files.listing_error = Some((path.clone(), error.to_string())),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn accept(files: &mut super::Files, value: String, cx: &mut Context<Root>) {
    files.completion.seen = (value.clone(), false);
    files
        .location
        .update(cx, |field, cx| field.set_text(value, cx));
}
