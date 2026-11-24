//! What the UI does when its host ends without this UI quitting it.
use crate::{
    app_host::AppHost, dialogs::dialog_button, host_client::HostEndpointConfig, theme::Palette,
};
use gpui::{
    App, Bounds, Context, Global, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use shellfox_service::local::{HostClosed, LocalClient};
use std::{ffi::OsString, time::Duration};

/// This UI's persistent arguments, for a restarted or reconnecting UI.
pub(crate) struct Relaunch(pub Vec<OsString>);
impl Global for Relaunch {}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// This UI is quitting the host itself.
    Expected,
    /// `host stop` or a service manager stopped the desktop's own host.
    Quit,
    /// The desktop's own host died with its shells: offer a fresh host.
    OfferRestart,
    /// A `host run` host went away: reconnect when it is back.
    AwaitHost,
}
fn outcome(owned: bool, closed: HostClosed, quitting: bool) -> Outcome {
    match (quitting, owned, closed) {
        (true, _, _) => Outcome::Expected,
        (false, true, HostClosed::Stopped) => Outcome::Quit,
        (false, true, HostClosed::Lost) => Outcome::OfferRestart,
        (false, false, _) => Outcome::AwaitHost,
    }
}

/// A live host answers a snapshot well within this.
const PING_DEADLINE: Duration = Duration::from_secs(10);
const PING_EVERY: Duration = Duration::from_secs(5);
/// Unanswered pings in a row before a host that keeps its lifetime open counts as lost.
const MISSED_PINGS: u32 = 3;

/// How the watched host ended.
#[derive(Debug, PartialEq, Eq)]
enum Ended {
    Closed(HostClosed),
    /// The host kept this UI's lifetime open but stopped answering.
    Unresponsive,
}

/// Resolves once `ping` failed `MISSED_PINGS` times in a row.
async fn unresponsive<F, P>(mut ping: P, every: Duration)
where
    P: FnMut() -> F,
    F: std::future::Future<Output = bool>,
{
    let mut missed = 0;
    while missed < MISSED_PINGS {
        tokio::time::sleep(every).await;
        missed = if ping().await { 0 } else { missed + 1 };
    }
}

/// Watches the host on a lifetime of its own for as long as the app runs, and
/// pings it, so a hung host ends the views as surely as one that exited.
pub(crate) fn watch(config: HostEndpointConfig, host: AppHost, cx: &mut App) {
    let (sender, ended) = tokio::sync::oneshot::channel();
    let watched = config.clone();
    let thread = std::thread::Builder::new()
        .name("shellfox-host-watch".into())
        .spawn(move || {
            let closed = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()
                .and_then(|runtime| {
                    runtime.block_on(async {
                        let client = LocalClient::connect(
                            &watched.data_dir,
                            &watched.helper,
                            watched.helper_sha256,
                        )
                        .await
                        .ok()?;
                        let ping = || async {
                            let snapshot = client
                                .execute(shellfox_service::local_api::LocalApiRequest::Snapshot {});
                            matches!(
                                tokio::time::timeout(PING_DEADLINE, snapshot).await,
                                Ok(Ok(_))
                            )
                        };
                        Some(tokio::select! {
                            closed = client.closed() => Ended::Closed(closed),
                            () = unresponsive(ping, PING_EVERY) => Ended::Unresponsive,
                        })
                    })
                });
            let _ = sender.send(closed.unwrap_or(Ended::Closed(HostClosed::Lost)));
        });
    if let Err(error) = thread {
        eprintln!("Shellfox host watch unavailable: {error:#}");
        return;
    }
    cx.spawn(async move |cx| {
        let Ok(closed) = ended.await else { return };
        let _ = cx.update(|cx| host_ended(closed, config, host, cx));
    })
    .detach();
}

fn host_ended(ended: Ended, config: HostEndpointConfig, host: AppHost, cx: &mut App) {
    let quitting = crate::host_process::quit_announced() || host.is_stopped().unwrap_or(true);
    let closed = match ended {
        Ended::Closed(closed) => closed,
        Ended::Unresponsive => {
            eprintln!("Shellfox host stopped answering.");
            // A hung host this UI started would keep its shells and block a
            // fresh host; end it so Restart starts one.
            crate::host_process::kill_started();
            HostClosed::Lost
        }
    };
    let outcome = outcome(!host.is_external(), closed, quitting);
    if outcome == Outcome::Expected {
        return;
    }
    // Nothing is left to stop, so quitting must not wait for this host.
    host.host_ended();
    match outcome {
        Outcome::Expected => {}
        Outcome::Quit => crate::app_exit::quit(cx),
        Outcome::OfferRestart => show(
            Notice {
                message: "Shellfox host stopped unexpectedly.",
                restart: true,
            },
            cx,
        ),
        Outcome::AwaitHost => {
            show(
                Notice {
                    message: "Shellfox host stopped. Reconnecting when it is back.",
                    restart: false,
                },
                cx,
            );
            reconnect_when_back(config, cx);
        }
    }
}

fn reconnect_when_back(config: HostEndpointConfig, cx: &mut App) {
    let executor = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            executor.timer(Duration::from_secs(1)).await;
            let probe = config.clone();
            let (sender, answered) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let answered = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .is_ok_and(|runtime| {
                        runtime.block_on(async {
                            let client = LocalClient::connect(
                                &probe.data_dir,
                                &probe.helper,
                                probe.helper_sha256,
                            )
                            .await;
                            if let Ok(client) = &client {
                                let _ = client.disconnect().await;
                            }
                            client.is_ok()
                        })
                    });
                let _ = sender.send(answered);
            });
            if answered.await.unwrap_or(false) {
                let _ = cx.update(relaunch);
                return;
            }
        }
    })
    .detach();
}

/// Starts a new UI process, which attaches to (or starts) a host, and quits this one.
fn relaunch(cx: &mut App) {
    let arguments = cx
        .try_global::<Relaunch>()
        .map(|relaunch| relaunch.0.clone())
        .unwrap_or_default();
    match std::env::current_exe()
        .and_then(|exe| std::process::Command::new(exe).args(arguments).spawn())
    {
        Ok(_) => crate::app_exit::quit(cx),
        Err(error) => eprintln!("Shellfox restart failed: {error:#}"),
    }
}

/// One small window replaces the windows whose views lost their host.
fn show(notice: Notice, cx: &mut App) {
    let options = WindowOptions {
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Shellfox".into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(380.), px(130.)),
            cx,
        ))),
        is_resizable: false,
        ..Default::default()
    };
    let stale = cx.windows();
    if let Err(error) = cx.open_window(options, |_, cx| cx.new(|_| notice)) {
        eprintln!("Shellfox host notice unavailable: {error:#}");
        crate::app_exit::quit(cx);
        return;
    }
    for handle in stale {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

struct Notice {
    message: &'static str,
    restart: bool,
}
impl Render for Notice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx
            .try_global::<crate::startup_chrome::StartupPalette>()
            .map(|colors| colors.palette)
            .unwrap_or_else(|| Palette::new("#ec4899", "#111016"));
        let mut buttons = div().flex().justify_end().gap(px(8.));
        if self.restart {
            buttons = buttons.child(
                dialog_button("host-restart", "Restart", palette).on_click(|_, _, cx| relaunch(cx)),
            );
        }
        buttons = buttons.child(
            dialog_button("host-quit", "Quit", palette)
                .on_click(|_, _, cx| crate::app_exit::quit(cx)),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .p(px(16.))
            .bg(palette.background)
            .text_color(palette.foreground)
            .text_size(px(12.))
            .child(self.message)
            .child(buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_end_outcomes() {
        assert_eq!(outcome(true, HostClosed::Lost, true), Outcome::Expected);
        assert_eq!(outcome(false, HostClosed::Lost, true), Outcome::Expected);
        assert_eq!(outcome(true, HostClosed::Stopped, false), Outcome::Quit);
        assert_eq!(
            outcome(true, HostClosed::Lost, false),
            Outcome::OfferRestart
        );
        assert_eq!(
            outcome(false, HostClosed::Stopped, false),
            Outcome::AwaitHost
        );
        assert_eq!(outcome(false, HostClosed::Lost, false), Outcome::AwaitHost);
    }

    #[tokio::test]
    async fn a_host_that_stops_answering_counts_as_lost() {
        let answers = [true, false, false, true, false, false, false];
        let pinged = std::cell::Cell::new(0);
        let ping = || {
            let answer = answers.get(pinged.get()).copied().unwrap_or(true);
            pinged.set(pinged.get() + 1);
            async move { answer }
        };
        unresponsive(ping, Duration::from_millis(1)).await;
        // One answer resets the count: only three misses in a row end the host.
        assert_eq!(pinged.get(), 7);
    }
}
