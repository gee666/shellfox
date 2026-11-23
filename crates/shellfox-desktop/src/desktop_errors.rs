//! Native startup/shutdown errors on Windows, even when no renderer exists.
#[cfg(windows)]
use windows::{
    Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
    core::HSTRING,
};

pub(crate) fn startup(error: &anyhow::Error) {
    let other = error
        .chain()
        .any(|cause| cause.is::<shellfox_service::local::OtherBuildRunning>());
    #[cfg(windows)]
    alert(
        "Shellfox",
        if other {
            "A different Shellfox version is still running. Close it first."
        } else {
            "The manager could not start. Check the installed application and data directory."
        },
    );
    #[cfg(not(windows))]
    let _ = other;
}

pub(crate) fn shutdown(cx: &gpui::App) -> bool {
    #[cfg(windows)]
    {
        cx.foreground_executor().spawn(async {
            alert("Shutdown failed", "Shutdown could not be confirmed. Shellfox is still open. Retry quit after pending work finishes.");
        }).detach();
        true
    }
    #[cfg(not(windows))]
    {
        let _ = cx;
        false
    }
}

#[cfg(windows)]
fn alert(title: &str, message: &str) {
    // Unparented native error boxes match Electron's showErrorBox. This call
    // runs outside any GPUI Window/App borrow and touches no unrelated window.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from(title),
            MB_OK | MB_ICONERROR,
        );
    }
}
