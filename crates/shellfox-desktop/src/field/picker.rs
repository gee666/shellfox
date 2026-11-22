use super::{Field, NumberKind};
use gpui::{App, Context, Entity, TextAlign};

impl Field {
    /// A compact centered input for the color popup. Numeric kinds step with Up/Down.
    pub(crate) fn color_input(number_kind: Option<NumberKind>, cx: &mut App) -> Entity<Self> {
        let field = Self::settings_input(number_kind, cx);
        field.update(cx, |field, _| {
            field.compact = true;
            field.text_align = TextAlign::Center;
            field.max_utf16_units = Some(if number_kind.is_some() { 3 } else { 7 });
        });
        field
    }

    pub(crate) fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.anchor = 0;
        self.set_cursor(self.text.len(), true);
        cx.notify();
    }
}
