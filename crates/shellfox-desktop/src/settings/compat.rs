use super::*;
use crate::root::Root;
impl Root {
    pub(crate) fn settings_page(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        match &self.settings.panel {
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
    pub(crate) fn review_access_requests(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.settings.panel else {
            return;
        };
        if matches!(
            self.dialogs.current,
            Some(crate::dialogs::Dialog::Create(_))
        ) && !self.dialog_locked()
        {
            self.dialogs.current = None;
        }
        panel.update(cx, |panel, cx| {
            panel.opened(cx);
            panel.focus_on_open = false;
            panel.category = Category::RemoteAccess;
            panel.access_review.set(true);
        });
        self.settings.open = true;
        cx.notify();
    }

    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = &self.settings.panel {
            if self.settings.open {
                panel.update(cx, |panel, cx| panel.request_close(cx));
                return;
            }
            panel.update(cx, |panel, cx| panel.opened(cx));
        }
        self.settings.open = !self.settings.open;
        cx.notify();
    }
    pub(crate) fn hide_settings(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.settings.open)
            && let Some(panel) = &self.settings.panel
        {
            panel.update(cx, |panel, cx| panel.dismiss(cx));
        }
    }
    pub(crate) fn sync_field_palette(&self, palette: Palette, cx: &mut Context<Self>) {
        let mut fields = Vec::new();
        match &self.dialogs.current {
            Some(crate::dialogs::Dialog::Create(draft)) => fields.push(draft.path.clone()),
            Some(crate::dialogs::Dialog::SessionEnvironment { text, .. }) => {
                fields.push(text.clone())
            }
            Some(crate::dialogs::Dialog::PeerSession(dialog)) => {
                fields.extend(dialog.field().cloned())
            }
            Some(
                crate::dialogs::Dialog::RenameSession { title, .. }
                | crate::dialogs::Dialog::RenameTab { title, .. },
            ) => fields.push(title.clone()),
            _ => {}
        }
        if let Some(files) = &self.files.panel {
            fields.push(files.location.clone());
            fields.extend(files.new_file.as_ref().map(|form| form.name.clone()));
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
        let views = self.tabs.views.values();
        let views = views.chain(self.peer_ui.v1_views.values());
        for view in views {
            if let Err(error) = view.update(cx, |view, cx| {
                view.apply_preferences(
                    &prefs.font_family,
                    prefs.font_size,
                    crate::terminal::TerminalTheme::from_colors(
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
