// AppKit requires the main thread, not the Rust test harness worker.
#[cfg(target_os = "macos")]
#[path = "../src/macos_folder_panel.rs"]
mod configuration;

fn main() {
    #[cfg(target_os = "macos")]
    {
        let main_thread = objc2::MainThreadMarker::new().expect("AppKit main thread");
        let _app = objc2_app_kit::NSApplication::sharedApplication(main_thread);
        let panel = configuration::folder_panel(main_thread);
        assert_eq!(
            panel.title().to_string(),
            "Choose default working directory"
        );
        assert!(panel.canChooseDirectories());
        assert!(!panel.canChooseFiles());
        assert!(!panel.allowsMultipleSelection());
    }
}
