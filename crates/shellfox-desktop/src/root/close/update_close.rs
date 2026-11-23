use super::*;
use anyhow::{Context as _, Result, ensure};
use gpui::{AnyWindowHandle, AsyncWindowContext, WindowHandle};

#[derive(Default)]
pub(super) struct OtherWindows(Vec<Participant>);
struct Participant {
    id: WindowId,
    handle: WindowHandle<Root>,
    frozen: bool,
}
impl OtherWindows {
    pub(super) fn inspect(own: AnyWindowHandle, cx: &gpui::App) -> Result<Self> {
        let participants = cx
            .windows()
            .into_iter()
            .filter(|handle| *handle != own)
            .filter_map(|handle| handle.downcast::<Root>())
            .map(|handle| {
                let root = handle.read(cx)?;
                ensure!(
                    !root.close.closing
                        && !root.close.check_pending
                        && !matches!(root.dialogs.current, Some(Dialog::Quit)),
                    "Wait for the other Shellfox window to finish closing."
                );
                let id = root
                    .close.window_id
                    .context("Wait for the other Shellfox window to open.")?;
                root.sync_app_guard(cx)?;
                Ok(Participant {
                    id,
                    handle,
                    frozen: false,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self(participants))
    }
    pub(super) fn check_targets(&self, own: Option<WindowId>, targets: &[WindowId]) -> Result<()> {
        ensure!(
            targets.len() == self.0.len() + usize::from(own.is_some())
                && targets
                    .iter()
                    .all(|id| Some(*id) == own || self.0.iter().any(|peer| peer.id == *id)),
            "Shellfox windows changed. Try closing again."
        );
        Ok(())
    }
    pub(super) fn drain(&mut self, cx: &mut AsyncWindowContext) -> Result<bool> {
        let mut ready = true;
        for peer in &mut self.0 {
            ready &= peer.handle.update(cx, |root, window, cx| {
                ensure!(root.close.window_id == Some(peer.id), "Shellfox window changed during quit.");
                if !peer.frozen {
                    ensure!(!root.close.closing && !root.close.check_pending && !root.block_close(cx),
                        "Save/discard files and await pending operations in the other windows before closing.");
                }
                let frozen = root.freeze_inputs(true, window, cx);
                peer.frozen = root.close.closing;
                Ok(frozen && root.drain_views(cx))
            })??;
        }
        Ok(ready)
    }
    pub(super) fn apply_success(
        &self,
        closed: &crate::app_host::ClosedWindows,
        cx: &mut AsyncWindowContext,
    ) -> bool {
        let mut all_closed = true;
        for peer in &self.0 {
            all_closed &= peer
                .handle
                .update(cx, |root, window, cx| {
                    root.close_confirmed(closed, window, cx)
                })
                .unwrap_or(false);
        }
        all_closed
    }
    pub(super) fn apply_failure(
        &self,
        retained: bool,
        withdrawing: bool,
        cx: &mut AsyncWindowContext,
    ) {
        for peer in self.0.iter().filter(|peer| peer.frozen) {
            let _ = peer.handle.update(cx, |root, window, cx| {
                root.resume_after_failed_close(retained, withdrawing, window, cx)
            });
        }
    }
}

impl Root {
    pub(super) fn close_confirmed(
        &mut self,
        closed: &ClosedWindows,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .close.window_id
            .is_some_and(|id| !closed.windows.contains(&id))
        {
            return false;
        }
        for view in self.tabs.views.values() {
            view.update(cx, |view, _| view.confirm_window_detach());
        }
        self.retire_all_peer_views(cx);
        self.host = None;
        self.close.window_id = None;
        self.close.app_host = None;
        self.close.closing = false;
        if self.block_close(cx) {
            return false;
        }
        window.remove_window();
        true
    }
    pub(super) fn resume_after_failed_close(
        &mut self,
        retained: bool,
        withdrawing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close.failed = true;
        if withdrawing {
            self.retire_all_peer_views(cx);
        }
        if !retained {
            self.freeze_inputs(false, window, cx);
            self.tabs.views.retain(|_, view| !view.read(cx).is_closed());
            for view in self.tabs.views.values() {
                view.update(cx, |view, _| view.resume_after_window_close());
            }
        }
        cx.notify();
    }
}
