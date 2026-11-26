//! A host that stops answering for a while is waited for, never ended: its
//! terminals and the programs in them are worth more than a responsive UI.
//! A small notice says so and goes away once the host answers again.
use crate::dialogs::dialog_button;
use gpui::{App, Context, Global, Window, WindowHandle, div, prelude::*, px};

/// Unanswered pings in a row before the notice shows.
pub(crate) const MISSED_PINGS: u32 = 3;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Stalled,
    Answering,
}

/// Counts unanswered pings; reports when the host stalls and when it is back.
#[derive(Default)]
pub(crate) struct Responsiveness {
    missed: u32,
}
impl Responsiveness {
    pub(crate) fn ping(&mut self, answered: bool) -> Option<Change> {
        let stalled = self.missed >= MISSED_PINGS;
        self.missed = if answered { 0 } else { self.missed + 1 };
        match (stalled, self.missed >= MISSED_PINGS) {
            (false, true) => Some(Change::Stalled),
            (true, false) => Some(Change::Answering),
            _ => None,
        }
    }
}

struct Shown(WindowHandle<Notice>);
impl Global for Shown {}

pub(crate) fn apply(change: Change, cx: &mut App) {
    match change {
        Change::Stalled => show(cx),
        Change::Answering => {
            eprintln!("Shellfox host answers again.");
            hide(cx);
        }
    }
}

fn show(cx: &mut App) {
    eprintln!("Shellfox host isn't answering; waiting for it.");
    if cx.has_global::<Shown>() {
        return;
    }
    // Only a host this UI started can be restarted from here.
    let restart = crate::host_process::started_host();
    let options = crate::host_lost::notice_options(cx);
    match cx.open_window(options, |_, cx| {
        cx.new(|_| Notice {
            restart,
            confirming: false,
        })
    }) {
        Ok(handle) => cx.set_global(Shown(handle)),
        Err(error) => eprintln!("Shellfox host notice unavailable: {error:#}"),
    }
}

/// Closes the notice, if it is still open.
pub(crate) fn hide(cx: &mut App) {
    if let Some(Shown(handle)) = cx.try_global::<Shown>().map(|shown| Shown(shown.0)) {
        cx.remove_global::<Shown>();
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

struct Notice {
    restart: bool,
    confirming: bool,
}
impl Render for Notice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = crate::host_lost::notice_palette(cx);
        let message = if self.confirming {
            "Restarting ends every terminal and the programs running in them."
        } else {
            "The Shellfox host isn't answering. Waiting for it…"
        };
        let mut buttons = div().flex().justify_end().gap(px(8.));
        if self.confirming {
            buttons = buttons
                .child(
                    dialog_button("host-stall-restart", "Restart", palette).on_click(|_, _, cx| {
                        // The host is hung, so it can't stop in order.
                        crate::host_process::announce_quit();
                        crate::host_process::kill_started();
                        crate::host_lost::relaunch(cx);
                    }),
                )
                .child(
                    dialog_button("host-stall-cancel", "Cancel", palette).on_click(cx.listener(
                        |notice, _, _, cx| {
                            notice.confirming = false;
                            cx.notify();
                        },
                    )),
                );
        } else {
            if self.restart {
                buttons = buttons.child(
                    dialog_button("host-stall-ask", "Restart…", palette).on_click(cx.listener(
                        |notice, _, _, cx| {
                            notice.confirming = true;
                            cx.notify();
                        },
                    )),
                );
            }
            buttons = buttons.child(
                dialog_button("host-stall-hide", "Hide", palette).on_click(|_, _, cx| hide(cx)),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .p(px(16.))
            .bg(palette.background)
            .text_color(palette.foreground)
            .text_size(px(12.))
            .child(message)
            .child(buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stall_shows_after_three_misses_and_clears_on_the_next_answer() {
        let mut host = Responsiveness::default();
        let changes: Vec<_> = [
            true, false, false, true, false, false, false, false, true, true,
        ]
        .into_iter()
        .map(|answered| host.ping(answered))
        .collect();
        assert_eq!(
            changes,
            [
                None,
                None,
                None,
                None,
                None,
                None,
                Some(Change::Stalled),
                None,
                Some(Change::Answering),
                None,
            ]
        );
    }
}
