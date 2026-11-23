//! The work that ends the app, run once on every OS: stop or leave the host,
//! then install a downloaded update. macOS exits inside GPUI's quit hook (NSApp
//! terminate), so the hook runs it. main runs it after the event loop only
//! when the hook never ran: a panic, or a failure before the loop started.
use crate::{app_startup::Prestarted, update::Updater};
use gpui::App;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Shellfox's own quit, after a confirmed close or when nothing needs one.
/// On macOS every other terminate request goes through the close confirmation.
pub(crate) fn quit(cx: &mut App) {
    QUIT_REQUESTED.store(true, Ordering::SeqCst);
    cx.quit();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Clean,
    StartupFailed,
    Panicked,
}

#[derive(Clone)]
pub(crate) struct Exit {
    owner: Option<Prestarted>,
    updater: Option<Arc<Updater>>,
    done: Rc<Cell<bool>>,
}
impl Exit {
    pub fn new(owner: Option<Prestarted>, updater: Option<Arc<Updater>>) -> Self {
        Self {
            owner,
            updater,
            done: Rc::default(),
        }
    }

    /// Registers the quit hook. Call first in the run callback, so that every
    /// quit path, including a startup failure, runs it.
    pub fn hook(&self, cx: &mut App) {
        let exit = self.clone();
        cx.on_app_quit(move |cx| {
            let failed = cx
                .try_global::<crate::app_startup::InitErrors>()
                .is_some_and(|errors| errors.0.borrow().is_some());
            exit.finish(if failed {
                Outcome::StartupFailed
            } else {
                Outcome::Clean
            });
            async {}
        })
        .detach();
    }

    /// Runs once. The update installs only after a clean run whose host is
    /// confirmed gone, since it replaces the binaries the host runs from.
    pub fn finish(&self, outcome: Outcome) {
        if self.done.replace(true) {
            return;
        }
        let host_gone = match &self.owner {
            Some(owner) => {
                crate::system_end::finish(&owner.host, outcome == Outcome::StartupFailed);
                owner.cleanup_until_confirmed();
                owner.host.is_stopped().unwrap_or(false)
            }
            None => true,
        };
        if installs_update(outcome, host_gone)
            && let Some(updater) = &self.updater
            && let Err(error) = updater.install_on_exit()
        {
            eprintln!("Shellfox update did not start: {error:#}");
        }
    }
}

fn installs_update(outcome: Outcome, host_gone: bool) -> bool {
    outcome == Outcome::Clean && host_gone
}

/// Dock Quit, a Cmd+Q from outside the app and AppleScript quit ask NSApp to
/// terminate. Unless Shellfox or the OS ending the session asked, that becomes
/// the menu's Quit, which confirms running terminals like closing the window.
/// Install before the event loop starts.
#[cfg(target_os = "macos")]
pub(crate) fn route_terminate() -> tokio::sync::mpsc::UnboundedReceiver<()> {
    let (ask, asked) = tokio::sync::mpsc::unbounded_channel();
    crate::macos_terminate::install(
        || QUIT_REQUESTED.load(Ordering::SeqCst) || crate::system_end::requested(),
        ask,
    );
    asked
}

#[cfg(target_os = "macos")]
pub(crate) fn answer_terminate(mut asked: tokio::sync::mpsc::UnboundedReceiver<()>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while asked.recv().await.is_some() {
            let _ = cx.update(|cx| {
                let windows = cx
                    .windows()
                    .iter()
                    .any(|window| window.downcast::<crate::root::Root>().is_some());
                if windows {
                    crate::app_menu::quit(cx)
                } else {
                    quit(cx)
                }
            });
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_update_installs_only_after_a_clean_run_without_a_host() {
        assert!(installs_update(Outcome::Clean, true));
        assert!(
            !installs_update(Outcome::Clean, false),
            "the host still runs from the old binaries"
        );
        assert!(!installs_update(Outcome::StartupFailed, true));
        assert!(!installs_update(Outcome::Panicked, true));
    }
    #[test]
    fn the_exit_work_runs_once() {
        let exit = Exit::new(None, None);
        assert!(!exit.done.get());
        exit.clone().finish(Outcome::Clean);
        assert!(exit.done.get());
        exit.finish(Outcome::Panicked);
    }
}
