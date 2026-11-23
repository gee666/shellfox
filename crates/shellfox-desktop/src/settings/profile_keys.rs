use super::{controls::Action, *};

fn move_profile(available: &[usize], current: usize, key: &str) -> Option<usize> {
    match key {
        "down" | "right" => available.iter().copied().find(|index| *index > current),
        "up" | "left" => available
            .iter()
            .copied()
            .rev()
            .find(|index| *index < current),
        "home" => available.first().copied(),
        "end" => available.last().copied(),
        _ => None,
    }
}

impl SettingsPanel {
    pub(super) fn shell_profiles(&self) -> impl Iterator<Item = &api::Profile> {
        self.catalog
            .iter()
            .flat_map(|catalog| &catalog.profiles)
            .filter(|profile| !profile.elevation)
    }

    pub(super) fn saved_shell_id(&self) -> Option<&str> {
        self.draft
            .as_ref()?
            .value
            .terminal_profile_id
            .as_ref()?
            .as_deref()
    }

    fn configured_shell_id(&self) -> Option<&str> {
        self.saved_shell_id()
            .or_else(|| self.catalog.as_ref()?.default_profile_id.as_deref())
    }

    /// The effective default, as 0.9 showed it: the saved one only if this host can run it.
    pub(super) fn selected_shell(&self) -> Option<&api::Profile> {
        let saved = self.configured_shell_id().unwrap_or_default();
        let usable = self
            .shell_profiles()
            .filter(|p| p.available)
            .map(|p| p.id.as_str());
        let id = shellfox_service::effective_default_profile(saved, usable)?;
        self.shell_profiles().find(|profile| profile.id == id)
    }

    pub(super) fn toggle_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.profile_menu {
            self.close_profiles(window, cx);
        } else if self.shell_profiles().next().is_some() {
            let selected = self.selected_shell().map(|profile| profile.id.as_str());
            let index = self
                .shell_profiles()
                .position(|profile| Some(profile.id.as_str()) == selected)
                .unwrap_or(0);
            self.profile_menu = true;
            self.profile_index = index;
            self.profile_scroll.scroll_to_item(index);
            cx.notify();
        }
    }

    pub(super) fn close_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.profile_menu = false;
        if let Some(focus) = self.controls.get("default-shell") {
            focus.focus(window);
        }
        cx.notify();
    }

    pub(super) fn select_profile(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.shell_profiles().any(|p| p.id == id && p.available) {
            return;
        }
        let changed = self.configured_shell_id() != Some(id.as_str());
        self.close_profiles(window, cx);
        if changed {
            self.stage(
                SettingsPatch {
                    terminal_profile_id: Some(Some(id)),
                    ..Default::default()
                },
                cx,
            );
        }
    }

    fn available_profile_id(&self, index: usize) -> Option<String> {
        self.shell_profiles()
            .nth(index)
            .filter(|profile| profile.available)
            .map(|profile| profile.id.clone())
    }

    pub(super) fn profile_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.frozen() || self.shell_profiles().next().is_none() {
            return;
        }
        let key = event.keystroke.key.as_str();
        let selected = self.selected_shell().map(|profile| profile.id.as_str());
        let current = if self.profile_menu {
            self.profile_index
        } else {
            self.shell_profiles()
                .position(|profile| Some(profile.id.as_str()) == selected)
                .unwrap_or(0)
        };
        let available = self
            .shell_profiles()
            .enumerate()
            .filter_map(|(index, profile)| profile.available.then_some(index))
            .collect::<Vec<_>>();
        let toggle = key == "f4" || event.keystroke.modifiers.alt && matches!(key, "down" | "up");
        if toggle || !self.profile_menu && matches!(key, "enter" | "space") {
            self.toggle_profiles(window, cx);
        } else if self.profile_menu && key == "escape" {
            self.close_profiles(window, cx);
        } else if self.profile_menu && matches!(key, "enter" | "tab") {
            // Chromium commits a popup selection on Tab without leaving the select.
            if let Some(id) = self.available_profile_id(current) {
                self.act(Action::SelectProfile(id), window, cx);
            } else {
                self.close_profiles(window, cx);
            }
        } else if let Some(index) = move_profile(&available, current, key) {
            if self.profile_menu {
                self.profile_index = index;
                self.profile_scroll.scroll_to_item(index);
            } else if let Some(id) = self.available_profile_id(index) {
                self.act(Action::SelectProfile(id), window, cx);
            }
        } else if !matches!(key, "up" | "down" | "left" | "right" | "home" | "end") {
            return;
        }
        self.keyboard_focus = true;
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::move_profile;

    #[test]
    fn shell_navigation_skips_unavailable_rows_and_stops_at_the_ends() {
        let rows = [1, 3, 5];
        assert_eq!(move_profile(&rows, 1, "down"), Some(3));
        assert_eq!(move_profile(&rows, 4, "left"), Some(3));
        assert_eq!(move_profile(&rows, 3, "home"), Some(1));
        assert_eq!(move_profile(&rows, 3, "end"), Some(5));
        assert_eq!(move_profile(&rows, 1, "up"), None);
        assert_eq!(move_profile(&rows, 5, "down"), None);
        assert_eq!(move_profile(&[], 0, "home"), None);
    }
}
