use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::NSOpenPanel;
use objc2_foundation::NSString;

pub(crate) fn folder_panel(main_thread: MainThreadMarker) -> Retained<NSOpenPanel> {
    let panel = NSOpenPanel::openPanel(main_thread);
    panel.setCanChooseFiles(false);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    panel.setTitle(Some(&NSString::from_str(
        "Choose default working directory",
    )));
    panel
}
