//! The part of a GPUI display not covered by the menu bar or the Dock. GPUI 0.2 only
//! exposes the full display; AppKit's `visibleFrame` has the rest.
use crate::window_manager::state::Screen;
use gpui::PlatformDisplay;
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSNumber, NSString};

pub(super) fn screen(display: &dyn PlatformDisplay) -> Screen {
    let mut screen = Screen::full(
        display.uuid().ok().map(|uuid| uuid.to_string()),
        display.bounds(),
    );
    if let Some(work) =
        MainThreadMarker::new().and_then(|mtm| visible(u32::from(display.id()), mtm))
    {
        screen.work = work;
    }
    screen
}

/// The visible frame of the NSScreen showing this CGDirectDisplayID, relative to its frame.
fn visible(display: u32, mtm: MainThreadMarker) -> Option<gpui::Bounds<gpui::Pixels>> {
    let key = NSString::from_str("NSScreenNumber");
    let ns = NSScreen::screens(mtm).to_vec().into_iter().find(|ns| {
        ns.deviceDescription()
            .objectForKey(&key)
            .and_then(|number| number.downcast::<NSNumber>().ok())
            .is_some_and(|number| number.unsignedIntValue() == display)
    })?;
    let rect = |rect: objc2_foundation::NSRect| {
        [
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        ]
    };
    super::appkit_work_area(rect(ns.frame()), rect(ns.visibleFrame()))
}
