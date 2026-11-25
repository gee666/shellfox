//! Native startup/shutdown errors, even when no renderer exists.

pub(crate) fn startup(error: &anyhow::Error) {
    let other = error
        .chain()
        .any(|cause| cause.is::<shellfox_service::local::OtherBuildRunning>());
    crate::native_dialog::alert(if other {
        "A different Shellfox version is still running. Close it first."
    } else {
        "The manager could not start. Check the installed application and data directory."
    });
}

pub(crate) fn shutdown(cx: &gpui::App) -> bool {
    #[cfg(windows)]
    {
        cx.foreground_executor().spawn(async {
            crate::native_dialog::alert("Shutdown could not be confirmed. Shellfox is still open. Retry quit after pending work finishes.");
        }).detach();
        true
    }
    #[cfg(not(windows))]
    {
        let _ = cx;
        false
    }
}
