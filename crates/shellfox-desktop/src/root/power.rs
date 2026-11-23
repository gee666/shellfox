//! Local Host-global keep-awake UI, never selected-peer/Manager state.
use super::*;
use crate::accessibility::{Role, SemanticAction, SemanticActionKind, SemanticNode};
use gpui::{AnyElement, IntoElement, prelude::*};
mod state;
pub(crate) use state::PowerUi;
const KEY: &str = "root/keep-awake";
impl Root {
    fn power_enabled(&self) -> bool {
        self.sidebar.visible
            && self.host.is_some()
            && !self.close.closing
            && !self.close.check_pending
            && self.dialogs.current.is_none()
            && !self.power.blocked()
    }
    pub(crate) fn render_power_button(
        &self,
        palette: crate::theme::Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let enabled = self.power_enabled();
        let mut button = crate::assets::toolbar_tip(
            crate::assets::icon_button(
                "keep-awake",
                if self.power.held() { "eye" } else { "eye-off" },
                palette,
            ),
            "Keep awake",
            palette,
        )
        .when(self.power.held(), |style| style.text_color(palette.accent))
        .when(!enabled, |style| style.opacity(0.5));
        if enabled {
            button =
                button.on_click(cx.listener(|root, _, window, cx| root.toggle_power(window, cx)));
        }
        let frame = self.semantics.context.frame();
        let Ok(id) = frame.id_for(KEY) else {
            return button.into_any_element();
        };
        let mut node = SemanticNode::new(
            id,
            Some(frame.root_id()),
            Role::Button,
            "Keep awake (local host)",
        );
        node.data.set_description(self.power.description());
        if let Some(toggled) = self.power.toggled() {
            node.data.set_toggled(toggled);
        }
        if enabled {
            node.data.add_action(accesskit::Action::Click);
        } else {
            node.data.set_disabled();
        }
        frame.wrap(node, button).into_any_element()
    }
    pub(crate) fn handle_power_action(
        &mut self,
        action: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Ok(id) = self.semantics.context.frame().id_for(KEY) else {
            return false;
        };
        if action.target != id {
            return false;
        }
        if matches!(action.kind, SemanticActionKind::Click) && self.power_enabled() {
            self.toggle_power(window, cx);
        }
        true
    }
    /// Follows the host-global keep-awake report, which other clients can change.
    pub(crate) fn watch_power(
        &self,
        client: crate::host_client::HostClient,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Task<()> {
        cx.spawn_in(window, async move |root, cx| {
            let mut changes = client.power_changes();
            while let Ok(report) = changes.next().await {
                let current = root.update(cx, |root, cx| {
                    if root.power.accept(report) {
                        cx.notify();
                    }
                    !root.close.closing
                });
                if !matches!(current, Ok(true)) {
                    return;
                }
            }
        })
    }
    pub(crate) fn toggle_power(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.power_enabled() {
            return;
        }
        let Some(client) = self.host.clone() else {
            return;
        };
        // The watched report is the host-global state, so the target is decided
        // from it, including ReleaseFailed -> retry OFF.
        let Some(target) = self.power.report.as_ref().and_then(state::confirmed_target) else {
            return;
        };
        let Some(request) = self.power.begin(true) else {
            return;
        };
        cx.spawn_in(window, async move |root, cx| {
            let result = client.set_power(target).await;
            let _ = root.update_in(cx, |root, _, cx| {
                if !root
                    .host
                    .as_ref()
                    .is_some_and(|current| current.same_window_client(&client))
                {
                    return;
                }
                let result = result.map_err(|error: anyhow::Error| {
                    format!("Local host keep-awake confirmation failed: {error:#}")
                });
                if root.power.complete(request, result) {
                    if let Some(error) = &root.power.error {
                        root.error = Some(error.clone());
                    } else if let Some(report) = &root.power.report {
                        if let Some(error) = &report.error {
                            root.error = Some(error.clone());
                        } else if let Some(reason) = &report.partial_reason {
                            root.error = Some(format!("Keep awake is partial: {reason}"));
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}
