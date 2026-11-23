//! At logout, restart and shutdown macOS posts a power-off notification, then
//! asks each app to quit. GPUI has no applicationShouldTerminate:, so that quit
//! ends the process right after applicationWillTerminate: (GPUI's on_app_quit).
//! The notification marks the end as a system end before it gets there.
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillPowerOffNotification};
use objc2_foundation::NSNotification;
use std::ptr::NonNull;

pub(super) fn listen(request: fn()) {
    let block = block2::RcBlock::new(move |_: NonNull<NSNotification>| request());
    // SAFETY: AppKit's own notification name, no sender filter, and a block
    // that only sets a flag and sends on a channel, which any thread may do.
    let observer = unsafe {
        NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceWillPowerOffNotification),
                None,
                None,
                &block,
            )
    };
    // Observes for the life of the app.
    std::mem::forget(observer);
}
