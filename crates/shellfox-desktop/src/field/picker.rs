use super::Field;
use gpui::{App, Context, Entity, TextAlign};

impl Field {
    pub(crate) fn color_channel(cx: &mut App) -> Entity<Self> {
        let field = Self::settings_input(None, cx);
        field.update(cx, |field, _| {
            field.font_size = 14.;
            field.preferred_font = Some("Arial".into());
            field.text_align = TextAlign::Center;
        });
        field
    }

    pub(crate) fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.anchor = 0;
        self.set_cursor(self.text.len(), true);
        cx.notify();
    }
}
