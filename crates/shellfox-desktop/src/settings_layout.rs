use super::*;
impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.semantic_actions.clear();
        self.semantic_fields.clear();
        let p = self.palette;
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
                Category::Ssh => self.ssh_page(cx),
            }
        };
        let body = self.semantic_element(
            &format!("panel/{}", self.category.label()),
            crate::accessibility::Role::TabPanel,
            self.category.label(),
            body,
        );
        let mut tabs = div()
            .h(px(34.))
            .flex_shrink_0()
            .px(px(20.))
            .flex()
            .gap(px(12.))
            .border_b_1()
            .border_color(p.border);
        for (index, category) in Category::ALL.into_iter().enumerate() {
            let button = div()
                .id(("settings-category", index))
                .track_focus(&self.tab_focus[index])
                .cursor_pointer()
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
                .when(self.category_error(category), |tab| {
                    tab.child(div().ml(px(5.)).text_color(gpui::rgb(0xfc8397)).child("•"))
                });
            let mut button = self
                .decorate_control(
                    &format!("category/{index}"),
                    category.label(),
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
            .track_focus(&self.close_focus)
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
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(720.))
                            .pt(px(22.))
                            .px(px(24.))
                            .pb(px(36.))
                            .text_size(px(12.))
                            .child(body),
                    ),
            );
        if self.environment.is_some() {
            page = page.child(self.environment_modal(window, cx));
        }
        self.semantic_root(page)
    }
}
