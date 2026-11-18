use super::{SemanticAction, SemanticError, SemanticTree, bridge::Bridge};
use gpui::{AnyWindowHandle, Window};
use std::{marker::PhantomData, rc::Rc, sync::atomic::Ordering};

#[path = "window_presentation.rs"]
mod presentation;
pub use presentation::{HiddenWindowPresentation, WindowPresentation};

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;

/// Own exactly one adapter per newly created GPUI window. Never accept a numeric PID/handle.
/// The OS callbacks own no GPUI objects. Dropping this object closes their action queue first.
pub struct NativeAccessibility {
    platform: platform::Provider,
    bridge: Bridge,
    window: AnyWindowHandle,
    previous: Option<SemanticTree>,
    presentation_started: bool,
    _main_thread: PhantomData<Rc<()>>,
}

impl NativeAccessibility {
    /// Call during open_window with show:false, focus:false, before Root focuses a field.
    /// macOS callers must also disable automatic window tabbing before window creation.
    pub fn attach(window: &Window) -> Result<Self, SemanticError> {
        let bridge = Bridge::default();
        let platform = platform::Provider::attach(window, bridge.clone())?;
        Ok(Self {
            platform,
            bridge,
            window: window.window_handle(),
            previous: None,
            presentation_started: false,
            _main_thread: PhantomData,
        })
    }

    /// Present once, only after hidden attachment and Root construction.
    /// Acceptance mode never activates a window or synthesizes focus/input.
    pub fn present_hidden(
        &mut self,
        window: &Window,
        plan: &HiddenWindowPresentation,
    ) -> Result<(), SemanticError> {
        if self.window != window.window_handle() || self.presentation_started {
            return Err(SemanticError::Native(
                "presentation requires the original unpresented window".into(),
            ));
        }
        #[cfg(target_os = "windows")]
        self.platform.validate_hidden(window)?;
        match plan.mode {
            WindowPresentation::Activate => window.activate_window(),
            WindowPresentation::WithoutActivation => {
                return Err(SemanticError::Native(
                    "nonactivating presentation must use queue_present_hidden outside GPUI borrows"
                        .into(),
                ));
            }
        }
        self.presentation_started = true;
        Ok(())
    }

    /// Queue acceptance-only native work on the UI dispatcher, without borrowing App/Window during show.
    /// Retain and await the returned task outside updates; completion verifies native and GPUI sizes,
    /// not pixels or accessibility activation. Dropping it cancels the queued attempt.
    pub fn queue_present_hidden(
        &mut self,
        window: &Window,
        plan: &HiddenWindowPresentation,
        cx: &gpui::App,
    ) -> Result<gpui::Task<Result<(), SemanticError>>, SemanticError> {
        if self.window != window.window_handle()
            || self.presentation_started
            || plan.mode != WindowPresentation::WithoutActivation
        {
            return Err(SemanticError::Native(
                "queue requires the original unpresented acceptance window".into(),
            ));
        }
        #[cfg(target_os = "windows")]
        {
            let bounds = plan.bounds.ok_or_else(|| {
                SemanticError::Native("missing explicit presentation bounds".into())
            })?;
            let task = self.platform.queue_without_activation(window, bounds, cx)?;
            self.presentation_started = true;
            Ok(task)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = cx;
            Err(SemanticError::Native(
                "nonactivating acceptance presentation is unsupported here".into(),
            ))
        }
    }

    /// Supply a complete tree after paint. Unchanged nodes produce no native notifications.
    /// Changes to terminal text must already be coalesced by TerminalDocument.
    pub fn publish(&mut self, tree: SemanticTree, window: &Window) -> Result<(), SemanticError> {
        if self.window != window.window_handle() {
            return Err(SemanticError::Native(
                "adapter belongs to another window".into(),
            ));
        }
        #[cfg(target_os = "windows")]
        self.platform.validate_window(window)?;
        self.platform.update_window(window);
        let force = self.bridge.needs_full.swap(false, Ordering::AcqRel);
        let native_changed = force
            || self
                .previous
                .as_ref()
                .is_none_or(|old| !tree.same_native_tree(old));
        if native_changed {
            let update = match &self.previous {
                Some(previous) if !force => tree.delta(previous),
                _ => tree.full_update(),
            };
            self.platform.update(update);
        }
        // Commit private action metadata even when native NodeData/delta is unchanged.
        self.bridge.replace(tree.clone());
        self.previous = Some(tree);
        Ok(())
    }

    /// Poll on the existing UI timer and request a paint when activation needs a full tree.
    pub fn needs_frame(&self) -> bool {
        self.bridge.needs_full.load(Ordering::Acquire)
    }

    /// Run on the UI thread and route through existing click/focus/edit handlers.
    /// Poll on the existing UI timer even when no new PTY output arrives.
    pub fn drain_actions(&self) -> Vec<SemanticAction> {
        self.bridge.drain()
    }
}

impl Drop for NativeAccessibility {
    fn drop(&mut self) {
        self.bridge.close();
    }
}
