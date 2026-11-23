use super::*;
#[path = "root_close_prompt.rs"]
mod prompt;
use crate::app_host::{
    CloseDecision, CloseTarget, ClosedWindows,
    WindowCloseGuard,
};
use std::time::{Duration, Instant};

fn host_operation_guard(local_pending: bool, peer_pending: bool) -> WindowCloseGuard {
    if local_pending || peer_pending {
        WindowCloseGuard::HostOperation
    } else {
        WindowCloseGuard::Ready
    }
}
#[cfg(test)]
#[path = "root_close_tests.rs"]
mod tests;
#[path = "root_close_update.rs"]
mod update_close;
use update_close::OtherWindows;

impl Root {
    fn window_guard(&self, cx: &gpui::App) -> WindowCloseGuard {
        match self.file_close_guard(cx) {
            crate::files::CloseGuard::Dirty => WindowCloseGuard::DirtyFile,
            crate::files::CloseGuard::Pending => WindowCloseGuard::FileOperation,
            crate::files::CloseGuard::Ready => host_operation_guard(
                self.user_action_pending
                    || self.power.blocks_close()
                    || !self.attaching.is_empty()
                    || self
                        .settings_panel
                        .as_ref()
                        .is_some_and(|panel| panel.read(cx).pending_operations()),
                self.peer_pending_operations(cx),
            ),
        }
    }
    pub(super) fn sync_app_guard(&self, cx: &gpui::App) -> anyhow::Result<()> {
        if !self.closing_window
            && let (Some(host), Some(id)) = (&self.app_host, self.window_id)
        {
            host.set_guard(id, self.window_guard(cx))?;
        }
        Ok(())
    }
    fn block_close(&mut self, cx: &mut Context<Self>) -> bool {
        let guard = self.window_guard(cx);
        if guard == WindowCloseGuard::Ready {
            return false;
        }
        if guard == WindowCloseGuard::DirtyFile {
            self.reveal_dirty_file_on_close(cx);
        } else if let Some(files) = &mut self.files {
            files.visible = true;
        }
        self.error = Some(
            match guard {
                WindowCloseGuard::DirtyFile => "Save or discard the edited file before closing.",
                WindowCloseGuard::FileOperation => {
                    "Wait for the file operation to finish before closing."
                }
                _ => "Wait for the current host operation to finish before closing.",
            }
            .into(),
        );
        cx.notify();
        true
    }
    fn freeze_inputs(&mut self, frozen: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(panel) = &self.settings_panel {
            if frozen && !panel.update(cx, |panel, cx| panel.try_freeze_for_close(cx)) {
                self.error = Some("Wait for settings operations to finish before closing.".into());
                cx.notify();
                return false;
            }
            if !frozen {
                panel.update(cx, |panel, cx| panel.resume_after_close_cancelled(cx));
            }
        }
        self.closing_window = frozen;
        if frozen {
            self.modal_focus.focus(window);
        }
        if let Some(editor) = self.files.as_ref().and_then(|files| files.editor.as_ref()) {
            editor
                .text
                .update(cx, |field, cx| field.set_read_only(frozen, cx));
        }
        let peers_ready = self.freeze_peer_inputs(frozen, cx);
        cx.notify();
        peers_ready
    }
    fn drain_views(&self, cx: &mut Context<Self>) -> bool {
        let mut done = true;
        for view in self.views.values().chain(self.fixture.iter()) {
            done &= view.update(cx, |view, _| view.drain_for_window_close());
        }
        done
    }
    pub(crate) fn begin_shutdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_check_pending
            || (self.closing_window && !self.close_failed)
            || self.block_close(cx)
        {
            return;
        }
        let Some(host) = self.app_host.clone() else {
            window.remove_window();
            return;
        };
        let mut others = if self.update_restart || self.system_end_requested() {
            match OtherWindows::inspect(window.window_handle(), cx) {
                Ok(windows) => windows,
                Err(error) => {
                    self.dismiss_dialog(window, cx);
                    self.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        } else {
            OtherWindows::default()
        };
        let Some(ticket) = self.close_confirmation.clone().map(|confirmation| confirmation.approve())
            .or_else(|| self.close_ticket.clone()) else { return; };
        // The prompt stays locked until the host reserves cleanup. Only then
        // can draining/detaching start, and failures use the cleanup error path.
        let prompted = matches!(self.dialog, Some(Dialog::Quit));
        self.close_failed = false;
        self.quit_focus.error = None;
        if !prompted {
            self.dialog = None;
            self.close_ticket = None;
        }
        let freeze_ready = self.freeze_inputs(true, window, cx);
        if !freeze_ready && !self.closing_window {
            if !prompted && !self.update_restart {
                self.close_ticket = Some(ticket);
            }
            return;
        }
        let own_id = self.window_id;
        let restart = self.update_restart;
        let restart_request = self.update_ui.restart_request.take();
        let updater = cx
            .try_global::<crate::update_view::Updates>()
            .map(|updates| updates.0.clone())
            .filter(|update| matches!(update.notice(), Some(crate::update::Notice::Ready)));
        let settings_client = self.host.clone();
        cx.spawn_in(window, async move |root, cx| {
            let mut admitted = false;
            let mut peer_withdrawal_started = false;
            let result = async {
                if !restart
                    && let (Some(updater), Some(client)) = (&updater, &settings_client)
                    && let Some(enabled) = crate::update_view::read_preference(client).await
                {
                    updater.preference(enabled);
                }
                let targets = host.begin_close(&ticket).await?;
                admitted = true;
                let _ = root.update_in(cx, |root, _, cx| {
                    root.dialog = None;
                    root.close_confirmation = None;
                    root.close_ticket = None;
                    root.update_restart = false;
                    cx.notify();
                });
                let drained = async {
                    others.check_targets(own_id, &targets)?;
                    let deadline = Instant::now() + Duration::from_secs(15);
                    loop {
                        let own_ready = root.update_in(cx, |root, window, cx| {
                            root.freeze_inputs(true, window, cx) && root.drain_views(cx)
                        })?;
                        if others.drain(cx)? && own_ready {
                            return Ok::<_, anyhow::Error>(());
                        }
                        anyhow::ensure!(
                            Instant::now() < deadline,
                            "Input releases/detach are still pending; windows were retained."
                        );
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;
                    }
                }
                .await;
                if let Err(error) = drained {
                    host.abort_close(&ticket)?;
                    return Err(error);
                }
                peer_withdrawal_started = true;
                host.finish_close(ticket.clone()).await
            }
            .await;
            match result {
                Ok(closed) => {
                    let others_closed = others.apply_success(&closed, cx);
                    let _ = root.update_in(cx, |root, window, cx| {
                        let own_closed = root.close_confirmed(&closed, window, cx);
                        if closed.app_stopped
                            && others_closed
                            && own_closed
                            && let Some(updater) = &updater
                        {
                            updater.confirm_close(restart);
                        }
                    });
                }
                Err(error) => {
                    let retained = host.cleanup_retained().unwrap_or(true);
                    if prompted && !admitted && !retained {
                        let decision = match host.owned_terminal_count().await {
                            Ok(count) => host.recheck_close(&ticket, count).ok(),
                            Err(_) => None,
                        };
                        let _ = root.update_in(cx, |root, window, cx| {
                            root.resume_after_failed_close(false, false, window, cx);
                            root.quit_admission_failed(error, decision, restart_request, window, cx);
                        });
                        return;
                    }
                    others.apply_failure(retained, peer_withdrawal_started, cx);
                    let _ = root.update_in(cx, |root, window, cx| {
                        root.resume_after_failed_close(
                            retained,
                            peer_withdrawal_started,
                            window,
                            cx,
                        );
                        let message = format!("Close failed; resources retained for retry: {error:#}");
                        if crate::desktop_errors::shutdown(cx) { eprintln!("{message}"); }
                        else { root.error = Some(message); }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }
}
