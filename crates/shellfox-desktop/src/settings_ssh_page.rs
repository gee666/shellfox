use super::ssh::Operation;
use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn ssh_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let import = self.text_button(
            "import-putty",
            "Import from PuTTY",
            Action::Ssh(Operation::Import),
            cx,
        );
        let add = self.text_button("add-ssh", "Add", Action::Ssh(Operation::Add), cx);
        let mut page = div().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .mb(px(8.))
                .child(controls::heading("SSH connections").mb_0())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(p.muted)
                                .child(self.ssh.status.clone().unwrap_or_default()),
                        )
                        .child(import)
                        .child(add),
                ),
        );
        let profiles = self.ssh.profiles.clone().unwrap_or_default();
        if self.ssh.editor.as_ref().is_some_and(|editor| editor.is_new) {
            page = page.child(self.ssh_form(cx));
        }
        for profile in &profiles {
            if self
                .ssh
                .editor
                .as_ref()
                .is_some_and(|editor| editor.id == profile.id)
            {
                page = page.child(self.ssh_form(cx));
                continue;
            }
            if self.ssh.removing.as_deref() == Some(profile.id.as_str()) {
                let remove = self
                    .text_button(
                        format!("confirm-remove-{}", profile.id),
                        "Remove",
                        Action::Ssh(Operation::ConfirmRemove(profile.id.clone())),
                        cx,
                    )
                    .text_color(gpui::rgb(0xfc8397));
                let cancel = self.text_button(
                    format!("cancel-remove-{}", profile.id),
                    "Cancel",
                    Action::Ssh(Operation::Cancel),
                    cx,
                );
                page = page.child(
                    div()
                        .min_h(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .border_b_1()
                        .border_color(p.border)
                        .child(div().flex_1().child(format!("Remove {}?", profile.name)))
                        .child(remove)
                        .child(cancel),
                );
                continue;
            }
            let address = format!(
                "{}{}{}",
                if profile.user.is_empty() {
                    String::new()
                } else {
                    format!("{}@", profile.user)
                },
                profile.host,
                if profile.port == 22 {
                    String::new()
                } else {
                    format!(":{}", profile.port)
                }
            );
            let copy = self
                .text_button(
                    format!("ssh-row-{}", profile.id),
                    "",
                    Action::Ssh(Operation::Edit(profile.id.clone())),
                    cx,
                )
                .h(px(28.))
                .min_h(px(28.))
                .flex_1()
                .min_w_0()
                .p_0()
                .justify_start()
                .gap(px(12.))
                .child(
                    div()
                        .w(gpui::relative(2. / 7.))
                        .overflow_hidden()
                        .child(profile.name.clone()),
                )
                .child(
                    div()
                        .w(gpui::relative(3. / 7.))
                        .overflow_hidden()
                        .text_size(px(11.))
                        .font_family("Cascadia Code")
                        .text_color(p.muted)
                        .child(address),
                )
                .child(
                    div()
                        .w(gpui::relative(2. / 7.))
                        .overflow_hidden()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(profile.remote_cwd.clone().unwrap_or_default()),
                );
            let edit = self
                .text_button(
                    format!("edit-ssh-{}", profile.id),
                    "",
                    Action::Ssh(Operation::Edit(profile.id.clone())),
                    cx,
                )
                .w(px(24.))
                .h(px(24.))
                .p_0()
                .child(crate::assets::icon("edit"));
            let remove = self
                .text_button(
                    format!("remove-ssh-{}", profile.id),
                    "",
                    Action::Ssh(Operation::Remove(profile.id.clone())),
                    cx,
                )
                .w(px(24.))
                .h(px(24.))
                .p_0()
                .child(crate::assets::icon("close"));
            page = page.child(
                div()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(p.border)
                    .child(copy)
                    .child(edit)
                    .child(remove),
            );
        }
        if self.ssh.profiles.is_some() && profiles.is_empty() && self.ssh.editor.is_none() {
            page = page.child(div().child("No connections yet."));
        }
        if self.ssh.profiles.is_none() && self.ssh.error.is_none() {
            page = page.child(controls::hint(
                if self.services.is_some() {
                    "Reading connections…"
                } else {
                    "SSH settings are unavailable on the connected host."
                },
                p,
            ));
        }
        if self.ssh.editor.is_none()
            && let Some(error) = &self.ssh.error
        {
            page = page.child(controls::hint(error.clone(), p).text_color(gpui::rgb(0xfc8397)));
        }
        page.child(
            div()
                .mt(px(8.))
                .text_size(px(11.))
                .text_color(p.muted)
                .flex()
                .gap(px(4.))
                .child("Connect from any terminal:")
                .child(div().font_family("Cascadia Code").child("shellfox ssh")),
        )
        .into_any_element()
    }
}
