//! The OS ending the desktop: logout, shutdown, SIGTERM or SIGHUP from a
//! service manager, or the display going away. As 0.9.x did, Shellfox then
//! quits without asking and a desktop-started host stops with its shells, so
//! the host neither keeps them nor relaunches the UI. Windows gets the same
//! from WM_ENDSESSION (accessibility/windows_end_session.rs).
use crate::{app_host::AppHost, root::Root};
use gpui::App;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

static ENDING: AtomicBool = AtomicBool::new(false);
static ASKED: Mutex<Option<oneshot::Sender<()>>> = Mutex::new(None);

/// The OS asked this UI to end.
pub(crate) fn requested() -> bool {
    ENDING.load(Ordering::SeqCst)
}

#[cfg(unix)]
fn request() {
    ENDING.store(true, Ordering::SeqCst);
    if let Some(asked) = ASKED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        let _ = asked.send(());
    }
}

/// Closes the windows as an approved Quit once the OS asks. app_exit.rs stops
/// the host if the app ends for an OS reason without having stopped it.
pub(crate) fn watch(cx: &mut App) {
    let (sender, asked) = oneshot::channel();
    *ASKED.lock().unwrap_or_else(|error| error.into_inner()) = Some(sender);
    listen();
    let executor = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        if asked.await.is_err() {
            return;
        }
        let _ = cx.update(close_windows);
        // An unsaved file or a stuck close must not hold up a logout.
        executor.timer(Duration::from_secs(10)).await;
        let _ = cx.update(crate::app_exit::quit);
    })
    .detach();
}

/// One window's system-end close is a Quit for every window (root_close_prompt.rs).
fn close_windows(cx: &mut App) {
    let Some(root) = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<Root>())
    else {
        return crate::app_exit::quit(cx);
    };
    let _ = root.update(cx, |root, window, cx| {
        if root.close(window, cx) {
            window.remove_window();
        }
    });
}

/// Runs once as the app ends (app_exit.rs).
/// `init_failed` means Shellfox quit on a startup error, not the OS.
pub(crate) fn finish(host: &AppHost, init_failed: bool) {
    let running = !host.is_external()
        && !host.is_stopped().unwrap_or(true)
        && !crate::host_process::quit_announced();
    if !stops_host(
        running,
        requested(),
        init_failed,
        shellfox_service::desktop_session::ended,
    ) {
        return;
    }
    let stopped = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(host.stop_for_system_end()));
    if let Err(error) = stopped {
        eprintln!("Shellfox host stop at session end failed: {error:#}");
    }
}

/// The event loop also ends when the display goes away, with no request.
fn stops_host(
    running: bool,
    requested: bool,
    init_failed: bool,
    session_ended: impl FnOnce() -> bool,
) -> bool {
    running && (requested || !init_failed && session_ended())
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod signals;

/// Windows windows see WM_ENDSESSION themselves.
fn listen() {
    #[cfg(unix)]
    signals::listen(request);
    #[cfg(target_os = "macos")]
    macos::listen(request);
}

#[cfg(test)]
mod tests {
    use super::stops_host;
    #[test]
    fn only_an_os_end_stops_a_running_host() {
        assert!(
            stops_host(true, true, true, || false),
            "a signal or power-off request"
        );
        assert!(
            stops_host(true, false, false, || true),
            "the display went away"
        );
        assert!(
            !stops_host(true, false, false, || false),
            "a crash-like end leaves the shells"
        );
        assert!(!stops_host(true, false, true, || panic!(
            "a startup error is not checked"
        )));
        assert!(
            !stops_host(false, true, false, || true),
            "stopped, quitting or `host run`"
        );
    }
}
