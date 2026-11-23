use super::*;

impl Root {
    pub(super) fn system_end_requested(&self) -> bool {
        crate::system_end::requested()
            || self
                .semantics.native
                .as_ref()
                .is_some_and(|native| native.system_end_requested())
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.close_with_update(window, cx, false)
    }

    pub(crate) fn close_with_update(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        restart: bool,
    ) -> bool {
        let system_end = self.system_end_requested();
        if self.close.check_pending
            || !system_end && matches!(self.dialogs.current, Some(Dialog::Quit))
            || (self.close.closing && !self.close.failed)
        {
            return false;
        }
        if self.block_close(cx) {
            return false;
        }
        let Some(host) = self.close.app_host.clone() else {
            return true;
        };
        // A failed additional window never owns another registered window's client.
        if self.close.window_id.is_none() && host.window_count().is_ok_and(|count| count != 0) {
            return true;
        }
        if let Err(error) = self.sync_app_guard(cx) {
            self.error = Some(error.to_string());
            cx.notify();
            return false;
        }
        if system_end && matches!(self.dialogs.current, Some(Dialog::Quit)) {
            self.dialogs.current = None;
            self.close.confirmation = None;
            self.close.ticket = None;
            self.update_ui.restart_request = None;
        }
        let restart = restart && !system_end;
        let restart_request = if restart {
            let Some(request) = cx
                .try_global::<crate::update::view::Updates>()
                .and_then(|updates| updates.0.request_restart())
            else {
                return false;
            };
            Some(request)
        } else {
            None
        };
        self.close.check_pending = true;
        if restart {
            cx.notify();
        }
        let placement = self.flush_window_bounds(window, cx);
        cx.spawn_in(window, async move |root, cx| {
            if let Some(write) = placement { write.finish().await; }
            let count = if system_end { Ok(0) } else { host.owned_terminal_count().await };
            let _ = root.update_in(cx, |root, window, cx| {
                root.close.check_pending = false;
                let system_end = system_end || root.system_end_requested();
                let count = match count {
                    _ if system_end => 0,
                    Ok(count) => count,
                    Err(error) => {
                        root.error = Some(format!("Owned terminal status is unavailable; cleanup is not confirmed: {error:#}"));
                        cx.notify();
                        return;
                    }
                };
                if root.block_close(cx) { return; }
                if let Err(error) = root.sync_app_guard(cx) {
                    root.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
                let restart = restart && !system_end;
                let target = if restart || system_end {
                    if let Err(error) = OtherWindows::inspect(window.window_handle(), cx) {
                        root.error = Some(error.to_string());
                        cx.notify();
                        return;
                    }
                    CloseTarget::Quit
                } else {
                    root.close.window_id.map(CloseTarget::Window).unwrap_or(CloseTarget::Quit)
                };
                match host.request_close(target, count) {
                    Ok(CloseDecision::Ready(ticket)) => {
                        root.close.update_restart = restart;
                        root.update_ui.restart_request = restart_request.filter(|_| restart);
                        root.close.ticket = Some(ticket);
                        if restart {
                            root.open_quit_dialog(window, cx);
                        } else {
                            root.begin_shutdown(window, cx);
                        }
                    }
                    Ok(CloseDecision::Confirm(confirmation)) => {
                        root.close.update_restart = restart;
                        root.update_ui.restart_request = restart_request;
                        root.close.confirmation = Some(confirmation);
                        root.open_quit_dialog(window, cx);
                    }
                    Ok(CloseDecision::Blocked(_)) => root.error = Some("Save/discard files and await pending operations before closing.".into()),
                    Err(error) => root.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }).detach();
        false
    }
}
