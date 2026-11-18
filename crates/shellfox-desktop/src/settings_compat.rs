use super::*;
use crate::root::Root;
impl Root {
    pub(crate) fn settings_page(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        match &self.settings_panel {
            Some(panel) => {
                panel.update(cx, |panel, cx| panel.set_palette(palette, cx));
                panel.clone().into_any_element()
            }
            None => div()
                .p(px(24.))
                .child("Reading settings…")
                .into_any_element(),
        }
    }
    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = !self.settings;
        if self.settings {
            if let Some(panel) = &self.settings_panel {
                panel.update(cx, |panel, cx| {
                    panel.opened(cx);
                });
            }
        } else if let Some(panel) = &self.settings_panel {
            panel.update(cx, |panel, cx| panel.request_close(cx));
        }
        cx.notify();
    }
    pub(crate) fn sync_field_palette(&self, palette: Palette, cx: &mut Context<Self>) {
        let mut fields = Vec::new();
        match &self.dialog {
            Some(crate::root::Dialog::Create(draft)) => fields.push(draft.path.clone()),
            Some(
                crate::root::Dialog::RenameSession { title, .. }
                | crate::root::Dialog::RenameTab { title, .. },
            ) => fields.push(title.clone()),
            _ => {}
        }
        if let Some(files) = &self.files {
            fields.push(files.location.clone());
            if let Some(editor) = &files.editor {
                fields.push(editor.text.clone());
            }
        }
        for field in fields {
            field.update(cx, |field, cx| field.apply_palette(palette, cx));
        }
    }
    pub(crate) fn apply_terminal_preferences(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prefs) = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.preferences.clone())
        else {
            return;
        };
        let palette = Palette::new(&prefs.accent_color, &prefs.background_color);
        #[cfg(feature="experimental-shellfox-native")]
        for view in self.peer_ui.views.values(){
            if let Err(error)=view.update(cx,|view,cx|view.apply_preferences(&prefs,window,cx)){self.error=Some(format!("Peer terminal preferences: {error:#}"));}
        }
        for view in self.views.values() {
            if let Err(error) = view.update(cx, |view, cx| {
                view.apply_preferences(
                    &prefs.font_family,
                    prefs.font_size,
                    crate::app::TerminalTheme::from_colors(
                        palette.background_rgb,
                        palette.foreground_rgb,
                        palette.accent_rgb,
                    )
                    .with_ansi_palette(palette.ansi),
                    window,
                    cx,
                )
            }) {
                self.error = Some(format!("Could not apply terminal preferences: {error:#}"));
            }
        }
    }
}
