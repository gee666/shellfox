use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn agents(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(rules) = self
            .draft
            .as_ref()
            .map(|draft| draft.value.process_rules.clone())
        else {
            return div().into_any_element();
        };
        let p = self.palette;
        let mut list = div();
        for rule in rules {
            let label = rules::builtin_label(&rule)
                .map(str::to_owned)
                .unwrap_or_else(|| rule.label.clone());
            let builtin = rules::builtin_label(&rule).is_some();
            let toggle = self.toggle(
                format!("track-{}", rule.id),
                rule.enabled,
                Action::ToggleAgent(rule.id.clone()),
                cx,
            );
            let mut copy = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .line_height(px(16.))
                .child(label);
            if !builtin {
                copy = copy.child(
                    div()
                        .text_size(px(10.))
                        .text_color(p.muted)
                        .child(rules::summary(&rule)),
                );
            }
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(toggle)
                .child(copy);
            if !builtin {
                row = row.child(
                    self.text_button(
                        format!("remove-agent-{}", rule.id),
                        "",
                        Action::RemoveAgent(rule.id),
                        cx,
                    )
                    .w(px(28.))
                    .h(px(28.))
                    .p_0()
                    .child(crate::assets::icon("close")),
                );
            }
            list = list.child(
                div()
                    .py(px(9.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(row),
            );
        }
        let field = self.bind_field(
            self.forms.fields[&Input::Agent].clone(),
            "Add agent",
            None,
            cx,
        );
        let add = self.button("add-agent", "Add", Action::AddAgent, cx);
        let mut page=div().child(controls::heading("Tracked programs"))
            .child(controls::hint("Tabs turn green while these programs run. Use a process name or an exact executable path.",p))
            .child(list)
            .child(div().mt(px(12.)).flex().gap(px(8.)).child(div().flex_1().min_w_0().on_key_down(cx.listener(|panel,event:&gpui::KeyDownEvent,_,cx|{if event.keystroke.key == "enter" && panel.control_available(&Action::AddAgent, cx) && !panel.forms.fields[&Input::Agent].read(cx).is_composing() { panel.add_agent(cx); cx.stop_propagation(); }})).child(field)).child(add));
        if let Some(error) = &self.agent_error {
            page = page.child(
                controls::hint(error.clone(), p)
                    .mt(px(4.))
                    .text_color(gpui::rgb(0xfc8397)),
            );
        }
        if let Some(error) = self.field_error(errors::Field::Agents) {
            page = page.child(controls::error(error));
        }
        page.into_any_element()
    }
}
