//! The sidebar notice of a UI that its host relaunched after a crash.
use crate::{accessibility::Role, assets::icon_sized, root::Root, theme::Palette};
use gpui::{AnyElement, Context, Global, div, prelude::*, px};
use std::path::Path;

/// About the report the host passed with `--crash-report`, until dismissed.
pub(crate) struct CrashNotice(String);
impl Global for CrashNotice {}

impl CrashNotice {
    pub fn new(report: &Path) -> Self {
        let cause = if shellfox_service::crash_report::is_gpu_failure(report) {
            "The GPU driver reset and Shellfox restarted."
        } else {
            "Shellfox closed unexpectedly."
        };
        Self(format!(
            "{cause} The crash report is in {}.",
            report.display()
        ))
    }
}

impl Root {
    pub(crate) fn crash_notice(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let text = cx.try_global::<CrashNotice>()?.0.clone();
        let dismiss = div()
            .id("dismiss-crash-notice")
            .flex_none()
            .cursor_pointer()
            .text_color(palette.muted)
            .hover(|style| style.text_color(palette.foreground))
            .child(icon_sized("close", 14.))
            .on_click(cx.listener(|_, _, window, cx| {
                // remove_global panics on a missing global, as after a double click.
                if cx.has_global::<CrashNotice>() {
                    cx.remove_global::<CrashNotice>();
                }
                window.refresh();
            }));
        let body = div()
            .id("sidebar-crash-notice")
            .flex_none()
            .w_full()
            .flex()
            .items_start()
            .gap(px(6.))
            .px(px(8.))
            .py(px(6.))
            .text_size(px(11.))
            .line_height(px(15.))
            .bg(palette.panel)
            .text_color(palette.muted)
            .border_t_1()
            .border_color(palette.border)
            .child(div().flex_1().min_w_0().child(text.clone()))
            .child(dismiss);
        Some(self.semantic_region("workspace/crash-notice", Role::Alert, text, body))
    }
}
