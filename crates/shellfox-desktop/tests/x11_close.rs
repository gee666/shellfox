//! Closing the X11 window as a window manager does (WM_DELETE_WINDOW) quits
//! Shellfox like 0.9.15: at once when no shell runs, after "Close shells and
//! quit" when one does. The UI exits cleanly and its host neither relaunches
//! it nor writes a crash report. GPUI used to run the close handler with its
//! X11 state borrowed, so every such close panicked. Runs only with
//! SHELLFOX_X11_CLOSE=1 under an X server with xdotool and python3, as in
//! linux-check.yml; see support/mod.rs for the other prerequisites.
mod support;

use std::{process::Command, time::Duration};
use support::{
    Desktop, Host, Ui, wait_exit,
    x11::{wait_for_shell, wait_until, window, xdotool},
};

/// Sends WM_PROTOCOLS/WM_DELETE_WINDOW to the window in argv[1], the message
/// a window manager sends for its close button. Xvfb runs no manager.
const DELETE_WINDOW: &str = r#"
import ctypes, sys
x = ctypes.cdll.LoadLibrary("libX11.so.6")
x.XOpenDisplay.restype = ctypes.c_void_p
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x.XInternAtom.restype = ctypes.c_ulong
class Message(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong),
                ("format", ctypes.c_int), ("data", ctypes.c_long * 5)]
class Event(ctypes.Union):
    _fields_ = [("xclient", Message), ("pad", ctypes.c_long * 24)]
x.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.POINTER(Event)]
display = x.XOpenDisplay(None)
assert display, "no X display"
event = Event()
event.xclient.type = 33  # ClientMessage
event.xclient.window = int(sys.argv[1])
event.xclient.message_type = x.XInternAtom(display, b"WM_PROTOCOLS", 0)
event.xclient.format = 32
event.xclient.data[0] = x.XInternAtom(display, b"WM_DELETE_WINDOW", 0)
assert x.XSendEvent(display, event.xclient.window, 0, 0, ctypes.byref(event))
x.XFlush(display)
"#;

fn delete_window(window: &str) {
    let status = Command::new("python3")
        .args(["-c", DELETE_WINDOW, window])
        .status()
        .unwrap();
    assert!(status.success(), "WM_DELETE_WINDOW not sent: {status}");
}

/// The UI exited 0, and after a while no host answers and no report exists.
fn quit_cleanly(desktop: &Desktop, ui: &mut Ui) {
    wait_exit(ui, Duration::from_secs(30), "The window close did not quit");
    let status = ui.0.wait().unwrap();
    assert!(status.success(), "The UI exited with {status}");
    // A relaunch would register a new window with a host that still answers.
    std::thread::sleep(Duration::from_secs(5));
    assert!(
        !desktop.host_answers(),
        "The host kept running or relaunched the UI"
    );
    let reports: Vec<_> = std::fs::read_dir(desktop.logs())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("crash-"))
        .collect();
    assert!(
        reports.is_empty(),
        "A window close was reported as a crash: {reports:?}"
    );
}

#[test]
fn closing_the_x11_window_quits_without_a_crash_or_a_relaunch() {
    if std::env::var_os("SHELLFOX_X11_CLOSE").is_none_or(|value| value != "1") {
        eprintln!("Skipped: set SHELLFOX_X11_CLOSE=1 under an X server with xdotool and python3");
        return;
    }
    // No shell: the window closes and Shellfox quits at once.
    let desktop = Desktop::new("desktop-x11-close-");
    let mut ui = Ui(desktop.command(&[]).spawn().unwrap());
    let host = Host(&desktop);
    desktop.wait_registered(Some(&mut ui));
    delete_window(&window());
    quit_cleanly(&desktop, &mut ui);
    drop(host);

    // One shell: the close asks first, and its "Close shells and quit" quits.
    let desktop = Desktop::new("desktop-x11-close-shell-");
    let folder = desktop.data.parent().unwrap().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    let mut ui = Ui(desktop.command(&[]).spawn().unwrap());
    let host = Host(&desktop);
    desktop.wait_registered(Some(&mut ui));
    desktop.forward(&["add", folder.to_str().unwrap()]);
    let window = window();
    wait_for_shell(&window, &folder, &mut ui);
    delete_window(&window);
    // The prompt holds the UI open; wait_until fails if it exits.
    wait_until(&mut ui, Duration::from_secs(3), || false);
    // The prompt focuses Cancel; Tab moves to "Close shells and quit".
    xdotool(&["windowfocus", "--sync", &window]);
    xdotool(&["key", "--delay", "200", "Tab", "Return"]);
    quit_cleanly(&desktop, &mut ui);
    drop(host);
}
