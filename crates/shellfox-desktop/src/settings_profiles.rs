use super::{controls::Action, *};
impl SettingsPanel {
    pub(super) fn profile_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let Some(catalog) = &self.catalog else {
            return;
        };
        let profiles = catalog
            .profiles
            .iter()
            .filter(|profile| profile.available && !profile.elevation)
            .map(|profile| profile.id.clone())
            .collect::<Vec<_>>();
        if profiles.is_empty() {
            return;
        }
        let selected = self
            .draft
            .as_ref()
            .and_then(|draft| draft.value.terminal_profile_id.as_ref())
            .and_then(Option::as_ref)
            .or(catalog.default_profile_id.as_ref());
        let current = if self.profile_menu {
            self.profile_index.min(profiles.len() - 1)
        } else {
            selected
                .and_then(|id| profiles.iter().position(|candidate| candidate == id))
                .unwrap_or(0)
        };
        let next = match key {
            "down" => Some((current + 1).min(profiles.len() - 1)),
            "up" => Some(current.saturating_sub(1)),
            "home" => Some(0),
            "end" => Some(profiles.len() - 1),
            _ => None,
        };
        if self.profile_menu {
            if let Some(next) = next {
                self.profile_index = next;
            } else if matches!(key, "enter" | "space") {
                self.act(Action::SelectProfile(profiles[current].clone()), window, cx);
            } else if key == "escape" {
                self.profile_menu = false;
            } else if key == "tab" {
                self.profile_menu = false;
                cx.notify();
                return;
            } else {
                return;
            }
        } else if (event.keystroke.modifiers.alt && matches!(key, "down" | "up"))
            || matches!(key, "enter" | "space" | "f4")
        {
            self.profile_index = current;
            self.profile_menu = true;
        } else if let Some(next) = next {
            self.act(Action::SelectProfile(profiles[next].clone()), window, cx);
        } else {
            return;
        }
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }
}
