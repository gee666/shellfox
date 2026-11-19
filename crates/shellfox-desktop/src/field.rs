use gpui::{App, Context, Entity, FocusHandle, Render, Window, prelude::*};
use std::ops::Range;

mod buffer;
mod document;
mod editor_line;
mod geometry;
mod history;
mod horizontal;
mod keyboard;
mod replacement;
mod native;
mod paint;
mod semantics;
mod snapshot;
mod picker;
mod stepper;
mod syntax;
pub(crate) use stepper::NumberKind;
mod text;
mod wrap;

#[derive(Default)]
pub(crate) struct FieldOptions {
    pub placeholder: Option<String>,
    pub read_only: Option<bool>,
    pub text_align: Option<gpui::TextAlign>,
}

/// Native editor used by forms and the bounded plain-text file editor.
pub struct Field {
    pub text: String,
    snapshot: snapshot::Snapshot,
    dirty_cache: snapshot::MatchCache,
    asset_editor: bool,
    syntax_language: Option<syntax::Language>,
    syntax_spans: std::sync::Arc<[syntax::Span]>,
    history: history::History,
    file_viewport: Option<f32>,
    nowrap: Option<horizontal::HorizontalLayout>,
    follow_horizontal: bool,
    focus: FocusHandle,
    selection: Range<usize>,
    cursor: usize,
    anchor: usize,
    dragging: bool,
    scroll_x: f32,
    pub(crate) scroll: gpui::ScrollHandle,
    marked: Option<Range<usize>>,
    multiline: bool,
    compact: bool,
    session_title: bool,
    settings_input: bool,
    max_utf16_units: Option<usize>,
    number_kind: Option<NumberKind>,
    wrap_width: Option<f32>,
    wrap_layout: Option<wrap::WrapLayout>,
    font_size: f32,
    preferred_font: Option<gpui::SharedString>,
    line_height: f32,
    monospace: bool,
    directory: bool,
    placeholder: String,
    text_align: gpui::TextAlign,
    code_editor: bool,
    read_only: bool,
    secret: bool,
    semantic: Option<semantics::Binding>,
    measured_semantics: Option<semantics::Measured>,
    painted_lines: Vec<geometry::PaintedLine>,
    palette: crate::theme::Palette,
}

impl Field {
    pub fn new(text: impl Into<String>, cx: &mut App) -> Entity<Self> {
        let text = text.into();
        let snapshot = snapshot::Snapshot::new(&text);
        cx.new(|cx| Self {
            snapshot,
            dirty_cache: snapshot::MatchCache::default(),
            asset_editor: false,
            syntax_language: None,
            syntax_spans: std::sync::Arc::new([]),
            history: Default::default(),
            file_viewport: None,
            nowrap: None,
            follow_horizontal: true,
            selection: text.len()..text.len(),
            cursor: text.len(),
            anchor: text.len(),
            dragging: false,
            scroll_x: 0.,
            scroll: gpui::ScrollHandle::new(),
            text,
            focus: cx.focus_handle(),
            marked: None,
            multiline: false,
            compact: false,
            session_title: false,
            settings_input: false,
            max_utf16_units: None,
            number_kind: None,
            wrap_width: None,
            wrap_layout: None,
            font_size: 13.,
            preferred_font: None,
            line_height: 20.,
            monospace: false,
            directory: false,
            placeholder: String::new(),
            text_align: gpui::TextAlign::Left,
            code_editor: false,
            read_only: false,
            secret: false,
            semantic: None,
            measured_semantics: None,
            painted_lines: Vec::new(),
            palette: crate::theme::Palette::new("#ec4899", "#111016"),
        })
    }

    pub(crate) fn apply_palette(&mut self, palette: crate::theme::Palette, cx: &mut Context<Self>) {
        if self.palette != palette {
            self.palette = palette;
            cx.notify();
        }
    }

    pub fn focus(&self, window: &mut Window) {
        self.focus.focus(window);
    }

    pub(crate) fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    pub(crate) fn edit_focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub(crate) fn settings_input(number_kind: Option<NumberKind>, cx: &mut App) -> Entity<Self> {
        let field = Self::new("", cx);
        field.update(cx, |field, _| {
            field.settings_input = true;
            field.font_size = 12.;
            field.number_kind = number_kind;
            if number_kind.is_some() {
                field.text_align = gpui::TextAlign::Right;
            }
        });
        field
    }

    pub(crate) fn limit_utf16(&mut self, units: usize) {
        self.max_utf16_units = Some(units);
    }

    pub(crate) fn tab_title(text: String, cx: &mut App) -> Entity<Self> {
        let field = Self::new(text, cx);
        field.update(cx, |field, _| {
            field.compact = true;
            field.font_size = 12.;
            field.line_height = 16.;
            field.anchor = 0;
            field.selection = 0..field.text.len();
        });
        field
    }

    pub(crate) fn session_title(text: String, cx: &mut App) -> Entity<Self> {
        let field = Self::new(text, cx);
        field.update(cx, |field, _| {
            field.session_title = true;
            field.line_height = 14.;
        });
        field
    }

    pub fn editor(text: String, cx: &mut App) -> Entity<Self> {
        let entity = Self::new(text, cx);
        entity.update(cx, |field, _| {
            field.multiline = true;
            field.monospace = true;
            field.font_size = 12.;
            field.line_height = 18.;
            field.code_editor = true;
        });
        entity
    }

    pub(crate) fn textarea(text: String, cx: &mut App) -> Entity<Self> {
        let entity = Self::editor(text, cx);
        entity.update(cx, |field, _| field.code_editor = false);
        entity
    }
    pub(crate) fn configure(&mut self, options: FieldOptions, cx: &mut Context<Self>) {
        let mut changed = false;
        if let Some(value) = options.placeholder
            && self.placeholder != value
        {
            self.placeholder = value;
            changed = true;
        }
        if let Some(value) = options.read_only
            && self.read_only != value
        {
            self.read_only = value;
            changed = true;
        }
        if let Some(value) = options.text_align
            && self.text_align != value
        {
            self.text_align = value;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }
    pub(crate) fn password(cx: &mut App) -> Entity<Self> {
        let entity = Self::settings_input(None, cx);
        entity.update(cx, |field, _| field.secret = true);
        entity
    }
    pub(crate) fn directory(cx: &mut App) -> Entity<Self> {
        let entity = Self::new("", cx);
        entity.update(cx, |field, _| {
            field.directory = true;
            field.monospace = true;
            field.line_height = 22.;
            field.placeholder = "folder path".into();
        });
        entity
    }
    pub(crate) fn file_path(text: String, cx: &mut App) -> Entity<Self> {
        let entity = Self::directory(cx);
        entity.update(cx, |field, cx| {
            field.font_size = 11.;
            field.set_text(text, cx);
        });
        entity
    }
    pub(crate) fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only != read_only {
            self.read_only = read_only;
            cx.notify();
        }
    }
    pub(crate) fn is_composing(&self) -> bool {
        self.marked.is_some()
    }
    pub(crate) fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.text = text;
        self.history = Default::default();
        self.refresh_snapshot();
        self.set_cursor(self.text.len(), false);
        self.marked = None;
        cx.notify();
    }

    fn click(&mut self, event: &gpui::MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus(window);
        self.history.break_group();
        if let Some(index) = paint::index_at(self, event.position, window) {
            self.set_cursor(index, event.modifiers.shift);
            self.dragging = true;
        }
        cx.notify();
    }

    fn set_cursor(&mut self, index: usize, extend: bool) {
        self.follow_horizontal |= self.cursor != index;
        if !extend {
            self.anchor = index;
        }
        self.cursor = index;
        self.selection = self.anchor.min(index)..self.anchor.max(index);
    }

    fn drag(&mut self, event: &gpui::MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.dragging
            && event.pressed_button == Some(gpui::MouseButton::Left)
            && let Some(index) = paint::index_at(self, event.position, window)
        {
            self.set_cursor(index, true);
            cx.notify();
        }
    }
    fn release(&mut self, _: &gpui::MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.dragging = false;
    }

    fn byte_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        Some(self.snapshot.byte_for_utf16(range.start)?..self.snapshot.byte_for_utf16(range.end)?)
    }

    fn utf16_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        Some(self.snapshot.utf16_for_byte(range.start)?..self.snapshot.utf16_for_byte(range.end)?)
    }


}

impl Render for Field {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_wrap(window);
        self.update_horizontal(window);
        paint::render(self, window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::buffer::byte_offset;
    use super::*;
    #[test]
    fn marked_composition_defers_editing_and_navigation_to_native_ime() {
        assert!(keyboard::native_owns_key(true, gpui::Modifiers::default()));
        assert!(!keyboard::native_owns_key(
            false,
            gpui::Modifiers::default()
        ));
    }
    #[test]
    fn altgr_never_invokes_destructive_clipboard_commands() {
        assert!(!keyboard::command_modifier(gpui::Modifiers {
            control: true,
            alt: true,
            ..Default::default()
        }));
    }
    #[test]
    fn native_offsets_never_split_surrogates() {
        assert_eq!(byte_offset("a😀b", 1), Some(1));
        assert_eq!(byte_offset("a😀b", 2), None);
        assert_eq!(byte_offset("a😀b", 3), Some(5));
        assert_eq!(byte_offset("a😀b", 5), None);
    }
}
