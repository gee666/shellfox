//! Existing new-terminal context menu, including saved SSH profile intents.
use crate::{chrome::button, root::Root, theme::Palette, workspace::AfterAction};
use gpui::{AnyElement, Context, Pixels, Point, Window, div, prelude::*, px};
use shellfox_core::{AddTab, ProfileId, SessionId};
use shellfox_host::HostRequest;
use shellfox_service::{
    native::{NativeAction, NativeResponse},
    ssh,
};

pub(crate) struct ProfileMenu {
    position: Point<Pixels>,
    session: SessionId,
    epoch: u64,
    choices: Vec<Choice>,
}
struct Choice {
    label: String,
    id: String,
    ssh: bool,
    available: bool,
}
impl Root {
    pub(crate) fn open_profile_menu(
        &mut self,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let epoch = self.navigation.epoch();
        let choices = self
            .profiles
            .iter()
            .map(|profile| Choice {
                label: profile.label.clone(),
                id: profile.id.clone(),
                ssh: false,
                available: profile.available,
            })
            .collect();
        self.profile_menu = Some(ProfileMenu {
            position,
            session,
            epoch,
            choices,
        });
        self.session_menu = None;
        if let Some(client) = self.host.clone() {
            cx.spawn_in(window, async move |root, cx| {
                let profiles = client.execute(NativeAction::Ssh(ssh::Request::List)).await;
                let _ = root.update_in(cx, |root, _, cx| {
                    if let Some(menu) = &mut root.profile_menu
                        && menu.session == session
                        && menu.epoch == epoch
                    {
                        match profiles {
                            Ok(NativeResponse::Ssh(ssh::Response::Profiles(profiles))) => menu
                                .choices
                                .extend(profiles.into_iter().map(|profile| Choice {
                                    label: profile.name,
                                    id: profile.id.to_string(),
                                    ssh: true,
                                    available: true,
                                })),
                            Ok(_) => root.error = Some("Invalid SSH profile menu response".into()),
                            Err(error) => {
                                root.error = Some(format!("SSH profiles unavailable: {error}"))
                            }
                        }
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }
    pub(crate) fn render_profile_menu(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(menu) = &self.profile_menu else {
            return div().into_any_element();
        };
        let viewport = window.viewport_size();
        let x = f32::from(menu.position.x).clamp(4., (f32::from(viewport.width) - 184.).max(4.));
        let y = f32::from(menu.position.y).clamp(4., (f32::from(viewport.height) - 220.).max(4.));
        let mut items = div()
            .id("terminal-profile-menu")
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(180.))
            .p_1()
            .rounded(px(6.))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.border)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|root, _, _, cx| {
                root.profile_menu = None;
                cx.notify();
            }));
        for (index, choice) in menu.choices.iter().enumerate() {
            let id = choice.id.clone();
            let ssh = choice.ssh;
            let available = choice.available;
            let session = menu.session;
            let epoch = menu.epoch;
            items = items.child(
                button(format!("profile-{index}"), choice.label.clone(), palette)
                    .w_full()
                    .justify_start()
                    .when(!available, |style| style.opacity(0.5))
                    .on_click(cx.listener(move |root, _, window, cx| {
                        root.profile_menu = None;
                        if !available || root.navigation.epoch() != epoch || root.closing_window {
                            cx.notify();
                            return;
                        }
                        if ssh {
                            root.open_saved_ssh(id.clone(), cx);
                        } else if let Ok(profile_id) = ProfileId::new(id.clone()) {
                            root.submit_native(
                                NativeAction::Metadata(HostRequest::AddTab {
                                    session_id: session,
                                    input: AddTab {
                                        title: None,
                                        cwd: None,
                                        profile_id,
                                    },
                                }),
                                AfterAction::NewTab,
                                window,
                                cx,
                            );
                        }
                        cx.notify();
                    })),
            );
        }
        items.into_any_element()
    }
}
