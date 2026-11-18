use super::*;
use crate::app_host::{
    ActivityShutdownState, CloseDecision, CloseTarget, ShutdownOutcome, WindowCloseGuard,
};
use std::time::{Duration, Instant};

// Persistence and presentation warnings do not undo confirmed resource cleanup.
pub(crate) fn log_shutdown_report(
    output: &mut impl std::io::Write,
    outcome: anyhow::Result<Option<ShutdownOutcome>>,
) {
    let message = match outcome {
        Ok(Some(outcome)) => {
            let message = match outcome.activity {
                ActivityShutdownState::Committed => "Final host close: session activity updates committed.".to_owned(),
                ActivityShutdownState::DirtyAfterOwnershipLoss { pending_sessions } => format!("Final host close: resources closed; {pending_sessions} session activity updates were not committed after ownership loss."),
            };
            let _ = writeln!(output, "{message}");
            if let shellfox_service::PresentationShutdownState::FailedSources(warnings) = &outcome.presentation {
                log_presentation_failures(output, warnings.failures().len());
            }
            // NoKnownFailure is not proof of final-view completeness.
            return;
        }
        Ok(None) => "Final host close: activity and presentation status are unknown; no shutdown report was available.".to_owned(),
        Err(error) => format!("Final host close: shutdown report could not be read; activity and presentation status are unknown: {error:#}"),
    };
    // A broken log sink cannot undo cleanup or turn an unknown report into a commit.
    let _ = writeln!(output, "{message}");
}
fn log_presentation_failures(output: &mut impl std::io::Write, count: usize) {
    // Opaque failed-source evidence stays with AppHost's typed report. Only count it here.
    let _ = writeln!(
        output,
        "Warning: host resources are confirmed closed; final presentation failed for {count} source(s). Opaque failure evidence remains retained."
    );
}

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
        if let Some(files) = &mut self.files {
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
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_check_pending || (self.closing_window && !self.close_failed) {
            return false;
        }
        if self.block_close(cx) {
            return false;
        }
        let Some(host) = self.app_host.clone() else {
            return true;
        };
        // A failed additional window never owns another registered window's client.
        if self.window_id.is_none() && host.window_count().is_ok_and(|count| count != 0) {
            return true;
        }
        if let Err(error) = self.sync_app_guard(cx) {
            self.error = Some(error.to_string());
            cx.notify();
            return false;
        }
        self.close_check_pending = true;
        cx.spawn_in(window,async move|root,cx|{
            let count=host.owned_terminal_count().await;
            let _=root.update_in(cx,|root,window,cx|{
                root.close_check_pending=false;
                let count=match count{Ok(count)=>count,Err(error)=>{root.error=Some(format!("Owned terminal status is unavailable; cleanup is not confirmed: {error:#}"));cx.notify();return;}};
                if root.block_close(cx){return;}
                if let Err(error)=root.sync_app_guard(cx){root.error=Some(error.to_string());cx.notify();return;}
                let target=root.window_id.map(CloseTarget::Window).unwrap_or(CloseTarget::Quit);
                match host.request_close(target,count){
                    Ok(CloseDecision::Ready(ticket))=>{root.close_ticket=Some(ticket);root.begin_shutdown(window,cx);},
                    Ok(CloseDecision::Confirm(confirmation))=>{root.close_confirmation=Some(confirmation);root.dialog=Some(Dialog::Quit);root.modal_focus.focus(window);},
                    Ok(CloseDecision::Blocked(_))=>root.error=Some("Save/discard files and await pending operations before closing.".into()),
                    Err(error)=>root.error=Some(error.to_string()),
                }cx.notify();
            });
        }).detach();
        false
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
            done &= view.update(cx, |view, _| view.close());
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
        let ticket = match self.close_confirmation.take() {
            Some(confirmation) => confirmation.approve(),
            None => match self.close_ticket.take() {
                Some(ticket) => ticket,
                None => return,
            },
        };
        self.close_failed = false;
        self.dialog = None;
        let freeze_ready = self.freeze_inputs(true, window, cx);
        if !freeze_ready && !self.closing_window {
            self.close_ticket = Some(ticket);
            return;
        }
        let own_id = self.window_id;
        cx.spawn_in(window, async move |root, cx| {
            let mut peer_withdrawal_started = false;
            let result = async {
                let targets = host.begin_close(&ticket).await?;
                anyhow::ensure!(
                    targets.iter().all(|id| Some(*id) == own_id),
                    "A window close cannot remove another window."
                );
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    if root.update_in(cx, |root, window, cx| {
                        // False may be a staged freeze: never cancel the original poll
                        // or retire a resumable SDK merely to make this drain pass.
                        root.freeze_inputs(true, window, cx) && root.drain_views(cx)
                    })? {
                        break;
                    }
                    if Instant::now() >= deadline {
                        host.abort_close(&ticket)?;
                        anyhow::bail!(
                            "Input releases/detach are still pending; the window was retained."
                        );
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                }
                peer_withdrawal_started = true;
                host.finish_close(ticket).await
            }
            .await;
            let _ = root.update_in(cx, |root, window, cx| {
                match result {
                    Ok(closed) => {
                        root.retire_all_peer_views(cx);
                        if closed.app_stopped {
                            log_shutdown_report(
                                &mut std::io::stderr().lock(),
                                host.shutdown_outcome(),
                            );
                        }
                        if root.window_id.is_none()
                            || root
                                .window_id
                                .is_some_and(|id| closed.windows.contains(&id))
                        {
                            root.host = None;
                            root.window_id = None;
                            root.app_host = None;
                            // No new edits were allowed after begin_close; still check pending file work.
                            root.closing_window = false;
                            if !root.block_close(cx) {
                                window.remove_window();
                            }
                        }
                    }
                    Err(error) => {
                        root.close_failed = true;
                        if peer_withdrawal_started {
                            // DetachAll may have disconnected Manager clients even if
                            // local detach failed. Old SDKs may not resume after that.
                            root.retire_all_peer_views(cx);
                        }
                        // Retained cleanup is not permission to resume metadata or stdin.
                        if !host.cleanup_retained().unwrap_or(true) {
                            root.freeze_inputs(false, window, cx);
                            root.views.retain(|_, view| !view.read(cx).is_closed());
                        }
                        root.error = Some(format!(
                            "Close failed; resources retained for retry: {error:#}"
                        ));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
