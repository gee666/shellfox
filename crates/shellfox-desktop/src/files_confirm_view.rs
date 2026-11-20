use super::text_button;
use crate::{files::pending::basename, root::Root, theme::Palette};
use gpui::{
    AnyElement, AnyWindowHandle, Context, FocusHandle, KeyDownEvent, Window, div, prelude::*, px,
};
use std::cell::{Cell, RefCell};

pub(crate) struct ConfirmationFocus {
    buttons: [FocusHandle; 3],
    previous: RefCell<Option<FocusHandle>>,
    active: Cell<Option<AnyWindowHandle>>,
}

impl ConfirmationFocus {
    pub fn new(cx: &mut gpui::App) -> Self {
        Self {
            buttons: std::array::from_fn(|_| cx.focus_handle()),
            previous: RefCell::default(),
            active: Cell::default(),
        }
    }

    pub(crate) fn forget(&self) {
        self.active.set(None);
        self.previous.borrow_mut().take();
    }

    pub(crate) fn end_panel(&self, cx: &mut Context<Root>) {
        let Some(handle) = self.active.get() else {
            return;
        };
        self.forget();
        let buttons = self.buttons.clone();
        let root = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = cx.update_window(handle, |_, window, cx| {
                if buttons.iter().any(|button| button.is_focused(window)) {
                    let _ = root.update(cx, |root, cx| root.focus_files_toggle(window, cx));
                }
            });
        });
    }

    fn restore(&self, fallback: &FocusHandle, window: &Window, cx: &mut gpui::App) {
        if self.active.take().is_none() {
            return;
        }
        let focus = self
            .previous
            .borrow_mut()
            .take()
            .unwrap_or_else(|| fallback.clone());
        let buttons = self.buttons.clone();
        window.defer(cx, move |window, _| {
            if buttons.iter().any(|button| button.is_focused(window)) {
                window.focus(&focus);
            }
        });
    }
}

impl Root {
    pub(super) fn file_confirmation(
        &self,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let files = self.files.as_ref()?;
        if files.pending_file_action.is_none() {
            files.confirm_focus.restore(&files.panel_focus, window, cx);
            return None;
        }
        let buttons = &files.confirm_focus.buttons;
        let busy = files.operation.is_some();
        if files
            .confirm_focus
            .active
            .replace(Some(window.window_handle()))
            .is_none()
        {
            *files.confirm_focus.previous.borrow_mut() = window.focused(cx);
        }
        let name = files.opened_path().map_or("this file", basename);
        let image_dirty = files
            .image_editor
            .as_ref()
            .is_some_and(crate::files::image_editor::ImageEditor::dirty);
        let can_save = !image_dirty
            && files
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.reload_required);
        let count = if can_save { 3 } else { 2 };
        if !buttons[..count]
            .iter()
            .any(|button| button.is_focused(window))
        {
            let cancel = buttons[0].clone();
            window.defer(cx, move |window, _| window.focus(&cancel));
        }
        let prompt = if image_dirty {
            "Save the image using its controls before continuing, or discard its changes.".into()
        } else {
            format!("Save changes to {name}?")
        };
        let keyboard = buttons.clone();
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .p(px(16.))
                .flex()
                .flex_col()
                .justify_center()
                .bg(palette.panel)
                .on_key_down(cx.listener(move |root, event: &KeyDownEvent, window, cx| {
                    let index = keyboard.iter().position(|button| button.is_focused(window));
                    let direction = match event.keystroke.key.as_str() {
                        "escape" => {
                            root.dismiss_file_action(cx);
                            0
                        }
                        "tab" if event.keystroke.modifiers.shift => count - 1,
                        "tab" => 1,
                        "space" | "enter" => {
                            match index {
                                Some(0) => root.dismiss_file_action(cx),
                                Some(1) => root.discard_and_continue_file_action(cx),
                                Some(2) => root.save_and_continue_file_action(cx),
                                _ => {}
                            }
                            0
                        }
                        _ => 0,
                    };
                    if direction != 0 && !busy {
                        if let Some(index) = index {
                            window.focus(&keyboard[(index + direction) % count]);
                        }
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                }))
                .child(div().child(prompt))
                .when_some(files.document_error.clone(), |dialog, error| {
                    dialog.child(
                        div()
                            .mt(px(6.))
                            .text_color(palette.status_color(0xf87171))
                            .child(error),
                    )
                })
                .when(busy, |dialog| {
                    dialog.child(
                        div()
                            .mt(px(6.))
                            .text_color(palette.muted)
                            .child("Saving file…"),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_end()
                        .gap(px(4.))
                        .mt(px(12.))
                        .child(
                            text_button("cancel-file-action", "Cancel", palette)
                                .track_focus(&buttons[0])
                                .when(busy, |button| button.opacity(0.4))
                                .on_click(
                                    cx.listener(|root, _, _, cx| root.dismiss_file_action(cx)),
                                ),
                        )
                        .child(
                            text_button("discard-file-action", "Discard changes", palette)
                                .track_focus(&buttons[1])
                                .when(busy, |button| button.opacity(0.4))
                                .on_click(cx.listener(|root, _, _, cx| {
                                    root.discard_and_continue_file_action(cx)
                                })),
                        )
                        .when(can_save, |row| {
                            row.child(
                                text_button("save-file-action", "Save and continue", palette)
                                    .track_focus(&buttons[2])
                                    .when(busy, |button| button.opacity(0.4))
                                    .on_click(cx.listener(|root, _, _, cx| {
                                        root.save_and_continue_file_action(cx)
                                    })),
                            )
                        }),
                )
                .into_any_element(),
        )
    }
}
