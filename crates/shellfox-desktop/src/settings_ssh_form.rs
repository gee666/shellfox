use super::ssh::Operation;
use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn ssh_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = &self.ssh.editor else {
            return div().into_any_element();
        };
        let name = editor.name.clone();
        let host = editor.host.clone();
        let port = editor.port.clone();
        let user = editor.user.clone();
        let password = editor.password.clone();
        let key = editor.key.clone();
        let cwd = editor.cwd.clone();
        let saved_password = editor.has_password && !editor.clear_password;
        let has_key = !key.read(cx).text.is_empty();
        let name = self.bind_field(name, "Name", None, cx);
        let host = self.bind_field(host, "Host", None, cx);
        let port = self.bind_field(port, "Port", None, cx);
        let user = self.bind_field(user, "User", None, cx);
        let password = self.bind_field(password, "Password", None, cx);
        let key = self.bind_field(key, "Key file", None, cx);
        let cwd = self.bind_field(cwd, "Remote folder", None, cx);
        let p = self.palette;
        let mut password_box = controls::column(1.).relative().child(password);
        if saved_password {
            password_box = password_box.child(
                self.text_button(
                    "clear-saved-password",
                    "clear",
                    Action::Ssh(Operation::ClearPassword),
                    cx,
                )
                .absolute()
                .right(px(2.))
                .top_0()
                .h(px(24.))
                .px(px(2.))
                .text_size(px(10.)),
            );
        }
        let choose = self
            .text_button("choose-ssh-key", "", Action::Ssh(Operation::ChooseKey), cx)
            .w(px(24.))
            .h(px(24.))
            .p_0()
            .child(crate::assets::icon("more"));
        let mut key_actions = div()
            .absolute()
            .right_0()
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .child(choose);
        if has_key {
            key_actions = key_actions.child(
                self.text_button("clear-ssh-key", "", Action::Ssh(Operation::ClearKey), cx)
                    .w(px(24.))
                    .h(px(24.))
                    .p_0()
                    .child(crate::assets::icon("close")),
            );
        }
        let cancel = self
            .text_button(
                "cancel-ssh-edit",
                "Cancel",
                Action::Ssh(Operation::Cancel),
                cx,
            )
            .h(px(24.))
            .min_h(px(24.))
            .px(px(8.))
            .py(px(2.));
        let save = self
            .button("save-ssh-edit", "Save", Action::Ssh(Operation::Save), cx)
            .h(px(26.))
            .py(px(2.))
            .bg(p.accent)
            .border_color(p.accent)
            .text_color(gpui::white());
        let mut form = div()
            .p(px(8.))
            .rounded(px(4.))
            .bg(p.accent.opacity(0.04))
            .flex()
            .flex_col()
            .on_key_down(
                cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
                    if panel.ssh.editor.as_ref().is_some_and(|editor| {
                        editor
                            .fields()
                            .iter()
                            .any(|field| field.read(cx).is_composing())
                    }) {
                        return;
                    }
                    if matches!(event.keystroke.key.as_str(), "enter" | "escape") {
                        let operation = if event.keystroke.key == "enter" {
                            Operation::Save
                        } else {
                            Operation::Cancel
                        };
                        panel.ssh_action(operation, window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(controls::column(1.).child(name))
                            .child(controls::column(2.).child(host))
                            .child(div().w(px(64.)).child(port)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(controls::column(1.).child(user))
                            .child(password_box)
                            .child(
                                controls::column(2.)
                                    .relative()
                                    .child(key)
                                    .child(key_actions),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(div().flex_1().min_w_0().child(cwd))
                            .child(cancel)
                            .child(save),
                    ),
            );
        if let Some(error) = &self.ssh.error {
            form = form.child(controls::error(error.clone()).line_height(px(15.)));
        }
        form.into_any_element()
    }
}
