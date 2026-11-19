use crate::{
    files::{
        Files,
        tree::{Branch, compare_entries},
    },
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};
use shellfox_files::{EntryKind, FileList};

pub(super) struct Rows<'a> {
    files: &'a Files,
    palette: Palette,
    elements: Vec<AnyElement>,
    keyboard: Vec<(usize, FocusHandle)>,
}

impl<'a> Rows<'a> {
    pub fn new(files: &'a Files, palette: Palette) -> Self {
        Self {
            files,
            palette,
            elements: Vec::new(),
            keyboard: Vec::new(),
        }
    }

    pub fn listing(&mut self, listing: &FileList, depth: usize, cx: &mut Context<Root>) {
        if listing.entries.is_empty() {
            self.push(message("Empty folder", self.palette));
        }
        let mut entries: Vec<_> = listing.entries.iter().collect();
        entries.sort_by(|a, b| compare_entries(a, b));
        for entry in entries {
            let kind = entry.kind;
            let branch = self
                .files
                .tree
                .branches
                .get(&entry.path)
                .map(|(_, branch)| branch);
            let supported = kind != EntryKind::Other;
            let mut focus = None;
            if supported {
                let handle = self
                    .files
                    .tree
                    .focus
                    .borrow_mut()
                    .entry(entry.path.clone())
                    .or_insert_with(|| cx.focus_handle())
                    .clone();
                focus = Some(handle.clone());
                self.keyboard.push((self.elements.len(), handle));
            }
            let selected = self.files.opened_path() == Some(entry.path.as_str());
            let path = entry.path.clone();
            let key_path = entry.path.clone();
            let open = branch.is_some();
            let palette = self.palette;
            let row = div()
                .id(gpui::SharedString::from(path.clone()))
                .h(px(24.))
                .w_full()
                .flex()
                .items_center()
                .gap(px(3.))
                .pl(px(8. + depth as f32 * 12.))
                .pr(px(6.))
                .text_size(px(11.))
                .when(selected, |style| {
                    style.bg(palette.hover).text_color(palette.accent)
                })
                .when(!supported, |style| style.opacity(0.4))
                .child(
                    div()
                        .w(px(12.))
                        .flex_shrink_0()
                        .text_center()
                        .text_color(palette.muted)
                        .child(match (kind, open) {
                            (EntryKind::Directory, true) => "▾",
                            (EntryKind::Directory, false) => "▸",
                            (EntryKind::Symlink, _) => "↗",
                            _ => "·",
                        }),
                )
                .child(div().min_w_0().truncate().child(entry.name.clone()))
                .when_some(focus, |row, focus| {
                    row.track_focus(&focus.clone().tab_stop(true))
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .pl(px(7. + depth as f32 * 12.))
                        .when(self.files.tree.keyboard_focus, |row| {
                            row.focus(|style| style.border_color(palette.accent))
                        })
                        .cursor_pointer()
                        .hover(|style| style.bg(palette.hover))
                        .tooltip(crate::assets::tooltip(entry.path.clone(), palette))
                        .on_click(cx.listener(move |root, _, window, cx| {
                            window.focus(&focus);
                            activate(root, kind, path.clone(), cx);
                        }))
                        .on_key_down(cx.listener(move |root, event: &KeyDownEvent, window, cx| {
                            if match event.keystroke.key.as_str() {
                                "left" => kind == EntryKind::Directory && open,
                                "right" => kind == EntryKind::Directory && !open,
                                _ => false,
                            } {
                                window.prevent_default();
                                cx.stop_propagation();
                                activate(root, kind, key_path.clone(), cx);
                            }
                        }))
                });
            self.push(row);
            match branch {
                Some(Branch::Loading) => self.push(message("Loading folder…", palette)),
                Some(Branch::Ready(children)) => self.listing(children, depth + 1, cx),
                Some(Branch::Failed(error)) => {
                    let path = entry.path.clone();
                    self.push(
                        message(error.clone(), palette).child(
                            super::super::text_button("retry-branch", "Retry folder", palette)
                                .on_click(cx.listener(move |root, _, _, cx| {
                                    root.load_file_branch(path.clone(), cx)
                                })),
                        ),
                    );
                }
                None => {}
            }
        }
        if listing.truncated {
            self.push(message(
                "Folder listing truncated. Open a subfolder or enter its path.",
                self.palette,
            ));
        }
    }

    pub fn push(&mut self, row: impl IntoElement) {
        self.elements.push(row.into_any_element());
    }

    pub fn render(self) -> AnyElement {
        let scroll = self.files.tree.scroll.clone();
        let scrollbar = self.files.tree.scrollbar.render(&scroll, self.palette);
        let tree = div()
            .id("file-tree")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .children(self.elements)
            .on_key_down(move |event: &KeyDownEvent, window: &mut Window, cx| {
                let Some(current) = self
                    .keyboard
                    .iter()
                    .position(|(_, focus)| focus.is_focused(window))
                else {
                    return;
                };
                let next = match event.keystroke.key.as_str() {
                    "down" => (current + 1).min(self.keyboard.len() - 1),
                    "up" => current.saturating_sub(1),
                    "home" => 0,
                    "end" => self.keyboard.len() - 1,
                    _ => return,
                };
                let (row, focus) = &self.keyboard[next];
                window.focus(focus);
                scroll.scroll_to_item(*row);
                window.prevent_default();
                cx.stop_propagation();
            });
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(tree)
            .child(scrollbar)
            .into_any_element()
    }
}

pub(super) fn message(text: impl Into<gpui::SharedString>, palette: Palette) -> gpui::Div {
    div()
        .px(px(8.))
        .py(px(5.))
        .text_color(palette.muted)
        .child(text.into())
}

fn activate(root: &mut Root, kind: EntryKind, path: String, cx: &mut Context<Root>) {
    match kind {
        EntryKind::Directory => root.toggle_file_branch(path, cx),
        EntryKind::File | EntryKind::Symlink => root.request_open_file(path, cx),
        EntryKind::Other => {}
    }
}
