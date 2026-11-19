use crate::{
    accessibility::Role,
    assets::{icon_sized, tooltip},
    host_client::HostClient,
    root::Root,
    theme::Palette,
    update::{Notice, RELEASES, RestartRequest, Updater},
};
use gpui::{AnyElement, App, Context, Global, Window, div, prelude::*, px};
use shellfox_service::native::{NativeAction, NativeResponse};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[path = "update_progress.rs"]
mod progress;

pub(crate) struct Updates(pub Arc<Updater>);
impl Global for Updates {}

pub(crate) fn preference(enabled: bool, cx: &App) {
    if let Some(updates) = cx.try_global::<Updates>() {
        updates.0.preference(enabled);
    }
}
pub(crate) async fn read_preference(client: &HostClient) -> Option<bool> {
    let response = client
        .execute(NativeAction::Settings(
            shellfox_host::SettingsRequest::Get {},
        ))
        .await
        .ok()?;
    match response {
        NativeResponse::Settings(settings) => Some(settings.auto_update),
        _ => None,
    }
}
pub(crate) fn read_initial_preference(client: HostClient, cx: &App) {
    if let Some(updates) = cx.try_global::<Updates>() {
        let updater = updates.0.clone();
        cx.spawn(async move |_| {
            if let Some(enabled) = read_preference(&client).await {
                updater.preference(enabled);
            }
        })
        .detach();
    }
}

#[derive(Default)]
pub(crate) struct UpdateUi {
    pub(crate) restart_request: Option<RestartRequest>,
    serial: u64,
    last_progress: Option<(semver::Version, Instant, u64)>,
    eta: Option<String>,
}
impl UpdateUi {
    fn poll(&mut self, serial: u64, notice: Option<Notice>, now: Instant) -> bool {
        let changed = std::mem::replace(&mut self.serial, serial) != serial;
        if let (Some(Notice::Downloading { version, .. }), Some((previous, at, _))) =
            (&notice, &self.last_progress)
            && previous == version
            && now.saturating_duration_since(*at) < Duration::from_millis(500)
        {
            return changed;
        }
        let old_eta = self.eta.clone();
        self.observe(notice, now);
        changed || self.eta != old_eta
    }

    fn observe(&mut self, notice: Option<Notice>, now: Instant) {
        let Some(Notice::Downloading {
            version,
            received,
            total,
        }) = notice
        else {
            self.last_progress = None;
            self.eta = None;
            return;
        };
        self.eta = self
            .last_progress
            .as_ref()
            .and_then(|(previous_version, then, previous)| {
                if total == 0
                    || *previous_version != version
                    || received <= *previous
                    || now <= *then
                {
                    return None;
                }
                let elapsed = now.duration_since(*then);
                let seconds = (u128::from(total.saturating_sub(received)) * elapsed.as_millis())
                    .div_ceil(u128::from(received - previous) * 1000);
                Some(if seconds < 60 {
                    format!("{seconds}s")
                } else {
                    format!("{}m", seconds.div_ceil(60))
                })
            });
        self.last_progress = Some((version, now, received));
    }
}

impl Root {
    pub(crate) fn poll_updates(&mut self, cx: &mut Context<Self>) {
        if let Some(updates) = cx.try_global::<Updates>() {
            let (serial, notice) = updates.0.notice_snapshot();
            if self.update_ui.poll(serial, notice, Instant::now()) {
                cx.notify();
            }
        }
    }

    pub(crate) fn update_notice(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let updater = cx.try_global::<Updates>()?.0.clone();
        let notice = updater.notice()?;
        let pane = div()
            .id("sidebar-update-notice")
            .flex_none()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(8.))
            .py(px(4.))
            .text_size(px(11.))
            .line_height(px(15.))
            .bg(palette.panel)
            .text_color(palette.muted)
            .border_t_1()
            .border_color(palette.border);
        let row = || {
            div()
                .flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .min_h(px(28.))
        };
        let brighten =
            |shift: u32| (((palette.accent_rgb >> shift & 0xffu32) * 5 + 2) / 4).min(255);
        let bright = gpui::rgb((brighten(16) << 16) | (brighten(8) << 8) | brighten(0));
        let symbol = |name: &'static str, enabled| {
            div()
                .id(name)
                .flex_none()
                .text_color(palette.accent)
                .when(enabled, |symbol| {
                    symbol.group_hover("update-action", |style| style.text_color(bright))
                })
                .child(icon_sized(name, 20.))
        };
        Some(
            match notice {
                Notice::Available {
                    version,
                    reason,
                    error,
                } => {
                    let hint = reason.map_or_else(
                        || format!("Download v{version}"),
                        |reason| format!("{reason} {RELEASES}"),
                    );
                    let button = div()
                        .id("download-update")
                        .group("update-action")
                        .flex_none()
                        .w(px(20.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .tooltip(tooltip(hint, palette))
                        .child(symbol("update-download", reason.is_none()))
                        .when(reason.is_some(), |button| button.opacity(0.4))
                        .when(reason.is_none(), |button| {
                            button
                                .cursor_pointer()
                                .on_click(move |_, _, _| updater.request_download())
                        });
                    pane.child(
                        row()
                            .child(
                                div()
                                    .flex()
                                    .gap(px(5.))
                                    .when(cfg!(windows), |label| label.relative().top(px(-1.)))
                                    .child(div().text_color(palette.accent).child(">."))
                                    .child("new version available"),
                            )
                            .child(button),
                    )
                    .when_some(error, |pane, error| {
                        pane.child(
                            self.semantic_region(
                                "workspace/update-error",
                                Role::Alert,
                                error.message(),
                                div()
                                    .text_size(px(10.))
                                    .line_height(px(13.5))
                                    .child(error.message()),
                            ),
                        )
                    })
                }
                Notice::Downloading {
                    received, total, ..
                } => pane.child(
                    div()
                        .flex()
                        .w_full()
                        .items_center()
                        .gap(px(6.))
                        .min_h(px(20.))
                        .child(progress::progress(received, total, palette))
                        .child({
                            let eta = self.update_ui.eta.as_deref().unwrap_or("…");
                            self.semantic_region(
                                "workspace/update-eta",
                                Role::Status,
                                format!("Estimated time remaining: {eta}"),
                                div()
                                    .id("update-eta")
                                    .flex_none()
                                    .text_size(px(9.))
                                    .tooltip(tooltip("Estimated time remaining", palette))
                                    .child(eta.to_owned()),
                            )
                        }),
                ),
                Notice::Ready => {
                    let available = self.host.is_some()
                        && !self.closing_window
                        && !self.close_check_pending
                        && !updater.install_pending();
                    pane.child(
                        row()
                            .id("install-update")
                            .group("update-action")
                            .tooltip(tooltip("Install now and restart Shellfox", palette))
                            .child(
                                div()
                                    .when(cfg!(windows), |label| label.relative().top(px(-1.)))
                                    .child("install and restart"),
                            )
                            .child(symbol("update-restart", available))
                            .when(!available, |button| button.opacity(0.4))
                            .when(available, |button| {
                                button
                                    .cursor_pointer()
                                    .hover(|style| style.text_color(palette.accent))
                                    .on_click(cx.listener(
                                        move |root, _, window: &mut Window, cx| {
                                            root.close_with_update(window, cx, true);
                                        },
                                    ))
                            }),
                    )
                }
            }
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eta_needs_increasing_progress_and_resets_at_phase_boundary() {
        let mut ui = UpdateUi::default();
        let then = Instant::now();
        let sample = |received| {
            Some(Notice::Downloading {
                version: "9.9.9".parse().unwrap(),
                received,
                total: 1000,
            })
        };
        assert!(ui.poll(1, sample(100), then));
        assert_eq!(ui.eta, None);
        assert!(ui.poll(2, sample(101), then + Duration::from_millis(250)));
        assert_eq!(ui.eta, None);
        assert!(ui.poll(3, sample(102), then + Duration::from_millis(500)));
        assert_eq!(ui.eta.as_deref(), Some("4m"));
        assert!(ui.poll(4, sample(700), then + Duration::from_secs(1)));
        assert_eq!(ui.eta.as_deref(), Some("1s"));
        // A stalled download does not publish a new worker revision.
        assert!(ui.poll(4, sample(700), then + Duration::from_millis(1500)));
        assert_eq!(ui.eta, None);
        assert!(ui.poll(5, Some(Notice::Ready), then + Duration::from_secs(2)));
        assert_eq!(ui.eta, None);
        assert_eq!(ui.last_progress, None);
    }
}
