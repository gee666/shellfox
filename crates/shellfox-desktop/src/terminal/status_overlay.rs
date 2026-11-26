use super::{Backend, TerminalView};
use gpui::{AnyElement, Context, EventEmitter, IntoElement, div, prelude::*, px, rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalRecovery {
    Retry,
    TakeControl,
}

impl EventEmitter<TerminalRecovery> for TerminalView {}

impl TerminalView {
    pub(super) fn render_status_overlay(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if !matches!(self.backend, Backend::Attached(_))
            || !(self.control_blocked
                || self.failed
                || self.screen.rows == 0
                || self.delivery.recovering)
        {
            return None;
        }
        let palette = crate::theme::Palette::new(
            &format!("#{:06x}", self.theme.accent),
            &format!("#{:06x}", self.theme.background),
        );
        if self.control_blocked {
            return Some(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(palette.background)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(palette.muted)
                    .child("Another client is controlling this terminal")
                    .child(
                        crate::chrome::button("take-terminal", "Take it", palette).on_click(
                            cx.listener(|_, _, _, cx| cx.emit(TerminalRecovery::TakeControl)),
                        ),
                    )
                    .into_any_element(),
            );
        }
        if self.failed {
            return Some(
                div()
                    .absolute()
                    .top(px(8.))
                    .right(px(12.))
                    .child(
                        crate::chrome::button("reconnect-terminal", "Reconnect terminal", palette)
                            .when(self.detached, |button| {
                                button.on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(TerminalRecovery::Retry)),
                                )
                            })
                            .when(!self.detached, |button| button.opacity(0.5)),
                    )
                    .into_any_element(),
            );
        }
        if self.screen.rows == 0 || self.delivery.recovering {
            return Some(
                div()
                    .absolute()
                    .inset_0()
                    .bg(rgb(self.theme.background))
                    .child(crate::renderer::terminal_loading(palette.accent))
                    .into_any_element(),
            );
        }
        None
    }
}
