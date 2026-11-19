//! Native startup/shutdown errors on Windows, even when no renderer exists.
#[cfg(windows)]
use windows::{
    Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
    core::HSTRING,
};

pub(crate) fn startup() {
    #[cfg(windows)]
    alert(
        "Shellfox",
        "The manager could not start. Check the installed application and data directory.",
    );
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
