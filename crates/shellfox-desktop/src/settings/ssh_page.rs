use super::ssh::Operation;
use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn ssh_page(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let import = self
            .text_button(
                "import-putty",
                "Import from PuTTY",
                Action::Ssh(Operation::Import),
                cx,
            )
            .h(px(24.))
            .min_h(px(24.));
        let add = self
            .text_button("add-ssh", "Add", Action::Ssh(Operation::Add), cx)
            .h(px(24.))
            .min_h(px(24.));
        let mut page = div().child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(8.))
                .mb(px(8.))
                .child(controls::heading("SSH connections").mb_0())
                .child(
                    div()
                        .h(px(26.))
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
        let mut previous_was_row = false;
        for profile in &profiles {
            if self
                .ssh
                .editor
                .as_ref()
                .is_some_and(|editor| editor.id == profile.id)
            {
                page = page.child(self.ssh_form(cx));
                previous_was_row = false;
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
                previous_was_row = false;
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
            let copy_id = ssh::row_id(&profile.id);
            let edit_id = format!("edit-ssh-{}", profile.id);
            let remove_id = format!("remove-ssh-{}", profile.id);
            let copy = self
                .text_button(
                    copy_id.clone(),
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
                    controls::column(1.)
                        .text_color(p.foreground)
                        .child(crate::assets::ellipsis(profile.name.clone(), 16.)),
                )
                .child(
                    controls::column(1.5)
                        .text_size(px(11.))
                        .font_family("Cascadia Code")
                        .text_color(p.muted)
                        .child(crate::assets::ellipsis(address, 16.)),
                )
                .child(
                    controls::column(1.)
                        .text_size(px(10.))
                        .text_color(p.muted)
                        .child(crate::assets::ellipsis(
                            profile.remote_cwd.clone().unwrap_or_default(),
                            16.,
                        )),
                );
            let edit = self
                .text_button(
                    edit_id.clone(),
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
                    remove_id.clone(),
                    "",
                    Action::Ssh(Operation::Remove(profile.id.clone())),
                    cx,
                )
                .w(px(24.))
                .h(px(24.))
                .p_0()
                .child(crate::assets::icon("close"));
            let focused = [&copy_id, &edit_id, &remove_id].into_iter().any(|id| {
                self.controls
                    .get(id)
                    .is_some_and(|focus| focus.is_focused(window))
            });
            let row = profile.id.clone();
            let show_actions = focused || self.ssh.hovered_row.as_ref() == Some(&row);
            page = page.child(
                div()
                    .id(gpui::SharedString::from(format!("ssh-actions-{copy_id}")))
                    .on_hover(cx.listener(move |panel, hovered: &bool, _, cx| {
                        if *hovered {
                            panel.ssh.hovered_row = Some(row.clone());
                        } else if panel.ssh.hovered_row.as_ref() == Some(&row) {
                            panel.ssh.hovered_row = None;
                        } else {
                            return;
                        }
                        cx.notify();
                    }))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .when(previous_was_row, |row| {
                        row.border_t_1().border_color(p.border)
                    })
                    .child(copy)
                    .child(
                        div()
                            .id("ssh-actions")
                            .flex()
                            .when(!show_actions, |actions| actions.invisible())
                            .child(edit)
                            .child(remove),
                    ),
            );
            previous_was_row = true;
        }
        if self.ssh.profiles.is_some() && profiles.is_empty() && self.ssh.editor.is_none() {
            page = page.child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(p.muted)
                    .child("No connections yet."),
            );
        }
        if let Some(error) = self
            .ssh
            .error
            .as_ref()
            .filter(|_| self.ssh.editor.is_none())
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
