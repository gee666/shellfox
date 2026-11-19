use super::*;
impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.semantic_actions.clear();
        self.semantic_fields.clear();
        let p = self.palette;
        let narrow = window.viewport_size().width <= px(560.);
        if self.focus_on_open {
            self.close_focus.focus(window);
            self.focus_on_open = false;
        }
        let body = if self.draft.is_none() {
            div()
                .child(
                    self.load_error
                        .clone()
                        .unwrap_or_else(|| "Reading settings…".into()),
                )
                .into_any_element()
        } else {
            match self.category {
                Category::General => self.general(cx),
                Category::Terminals => self.terminals(cx),
                Category::Agents => self.agents(cx),
                Category::RemoteAccess => self.remote_page(cx),
                Category::Ssh => self.ssh_page(window, cx),
            }
        };
        let restore_environment_focus = std::mem::take(&mut self.restore_environment_focus);
        if restore_environment_focus && self.category == Category::Terminals {
            if let Some(trigger) = self.controls.get("all-terminal-environment") {
                trigger.focus(window);
            }
        }
        let ssh_focus = self.ssh.focus_control.take();
        if self.category == Category::Ssh {
            if let Some(focus) = ssh_focus.as_ref().and_then(|id| {
                self.controls
                    .get(id)
                    .or_else(|| self.controls.get("add-ssh"))
            }) {
                focus.focus(window);
            }
        }
        let body = self.semantic_element(
            &format!("panel/{}", self.category.label()),
            crate::accessibility::Role::TabPanel,
            self.category.label(),
            body,
        );
        let mut tabs = div()
            .id("settings-categories")
            .track_scroll(&self.tabs_scroll)
            .overflow_x_scroll()
            .h(px(34.))
            .flex_shrink_0()
            .px(px(if narrow { 12. } else { 20. }))
            .flex()
            .gap(px(if narrow { 10. } else { 12. }))
            .border_b_1()
            .border_color(p.border);
        for (index, category) in Category::ALL.into_iter().enumerate() {
            let invalid = self.category_error(category);
            let error_label = invalid.then(|| format!("{}, needs attention", category.label()));
            let button = div()
                .id(("settings-category", index))
                .track_focus(
                    &self.tab_focus[index]
                        .clone()
                        .tab_stop(self.category == category),
                )
                .flex_shrink_0()
                .cursor_pointer()
                .hover(move |style| style.text_color(p.foreground))
                .relative()
                .when(self.keyboard_focus, |element| {
                    element.child(controls::focus_outline(
                        self.tab_focus[index].clone(),
                        p.accent,
                        0.,
                        0.,
                    ))
                })
                .pt(px(7.))
                .pb(px(8.))
                .px(px(2.))
                .text_size(px(12.))
                .text_color(if self.category == category {
                    p.foreground
                } else {
                    p.muted
                })
                .when(self.category == category, |style| {
                    style.border_b_2().border_color(p.accent)
                })
                .on_click(cx.listener(move |panel, _, window, cx| {
                    panel.act(controls::Action::SelectCategory(category), window, cx);
                }))
                .on_key_down(
                    cx.listener(move |panel, event: &gpui::KeyDownEvent, window, cx| {
                        let next = match event.keystroke.key.as_str() {
                            "right" => Some((index + 1) % 5),
                            "left" => Some((index + 4) % 5),
                            "home" => Some(0),
                            "end" => Some(4),
                            _ => None,
                        };
                        if let Some(next) = next {
                            panel.keyboard_focus = true;
                            panel.act(
                                controls::Action::SelectCategory(Category::ALL[next]),
                                window,
                                cx,
                            );
                            window.prevent_default();
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }),
                )
                .flex()
                .items_center()
                .child(category.label())
                .when_some(
                    self.remote
                        .state
                        .as_ref()
                        .map(|state| state.pending.len())
                        .filter(|count| category == Category::RemoteAccess && *count > 0),
                    |tab, count| {
                        tab.child(
                            div()
                                .ml(px(5.))
                                .px(px(4.))
                                .border_1()
                                .border_color(p.border)
                                .rounded(px(3.))
                                .text_size(px(9.))
                                .line_height(px(12.))
                                .text_color(p.accent)
                                .child(count.to_string()),
                        )
                    },
                )
                .when(invalid, |tab| {
                    tab.child(div().ml(px(5.)).text_color(gpui::rgb(0xfc8397)).child("•"))
                });
            let mut button = self
                .decorate_control(
                    &format!("category/{index}"),
                    error_label.as_deref().unwrap_or(category.label()),
                    controls::Action::SelectCategory(category),
                    self.tab_focus[index].clone(),
                    button,
                )
                .role(crate::accessibility::Role::Tab)
                .selected(self.category == category);
            if let Some(parent) = self.semantic_id("categories") {
                button = button.parent(parent);
            }
            tabs = tabs.child(button);
        }
        let tabs = self.semantic_element(
            "categories",
            crate::accessibility::Role::TabList,
            "Settings categories",
            tabs,
        );
        let close = crate::assets::icon_button("close-settings-panel", "close", p)
            .tooltip(crate::assets::tooltip("Close settings (Esc)", p))
            .track_focus(&self.close_focus.clone().tab_stop(true))
            .relative()
            .when(self.keyboard_focus, |element| {
                element.child(controls::focus_outline(
                    self.close_focus.clone(),
                    p.accent,
                    4.,
                    0.,
                ))
            })
            .on_key_down(
                cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        panel.act(controls::Action::CloseSettings, window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }),
            )
            .on_click(cx.listener(|panel, _, window, cx| {
                panel.act(controls::Action::CloseSettings, window, cx)
            }));
        let mut close = self.decorate_control(
            "close-settings-panel",
            "Close settings",
            controls::Action::CloseSettings,
            self.close_focus.clone(),
            close,
        );
        if let Some(parent) = self.semantic_root_id() {
            close = close.parent(parent);
        }
        let status = div()
            .text_size(px(11.))
            .text_color(p.muted)
            .child(self.status());
        let status = if self.status().is_empty() {
            status.into_any_element()
        } else {
            self.semantic_element(
                "save-status",
                crate::accessibility::Role::Status,
                self.status(),
                status,
            )
        };
        let mut page = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .capture_any_mouse_down(cx.listener(|panel, _, _, cx| {
                if std::mem::take(&mut panel.keyboard_focus) {
                    cx.notify();
                }
            }))
            .on_key_down(
                cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
                    panel.key(event, window, cx)
                }),
            )
            .child(
                div()
                    .h(px(36.))
                    .flex_shrink_0()
                    .pl(px(12.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(p.sidebar)
                    .border_b_1()
                    .border_color(p.border)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(10.))
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Settings"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child("This device"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(status)
                            .child(close),
                    ),
            )
            .child(tabs)
            .child(
                div()
                    .id("settings-scroll")
                    .track_scroll(&self.body_scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(720.))
                            .pt(px(if narrow { 18. } else { 22. }))
                            .px(px(if narrow { 14. } else { 24. }))
                            .pb(px(if narrow { 28. } else { 36. }))
                            .text_size(px(12.))
                            .child(body),
                    ),
            );
        if self.environment.is_some() {
            page = page.child(self.environment_modal(window, cx));
        }
        if let Some(picker) = &self.color_picker {
            page = page.child(picker.view.clone());
        }
        self.semantic_root(page)
    }
}
