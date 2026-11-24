//! Typing into a real X11 window with xdotool.
use super::Ui;
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

pub fn xdotool(args: &[&str]) -> String {
    let output = Command::new("xdotool").args(args).output().unwrap();
    assert!(output.status.success(), "xdotool {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// The first Shellfox window, once it is mapped.
pub fn window() -> String {
    let window = xdotool(&["search", "--sync", "--name", "^Shellfox v"]);
    window.lines().next().unwrap().to_owned()
}

/// Replaces the shell's line with `line` and presses Return. xdotool types
/// "\n" as the Linefeed keysym, which is not Enter.
pub fn type_line(window: &str, line: &str) {
    xdotool(&["windowfocus", "--sync", window]);
    xdotool(&["key", "ctrl+u"]);
    xdotool(&["type", "--delay", "20", line]);
    xdotool(&["key", "Return"]);
}

/// Waits up to `within` for `done`; fails if the desktop exits first.
pub fn wait_until(ui: &mut Ui, within: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        if let Some(status) = ui.0.try_wait().unwrap() {
            panic!("The desktop exited: {status}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    done()
}

/// Retypes a probe until the shell in `folder` runs it: the first keys can
/// arrive before a new tab's terminal has focus.
pub fn wait_for_shell(window: &str, folder: &Path, ui: &mut Ui) {
    let marker = folder.join("shell-ready");
    let _ = std::fs::remove_file(&marker);
    for _ in 0..6 {
        type_line(window, &format!("touch '{}'", marker.display()));
        if wait_until(ui, Duration::from_secs(5), || marker.exists()) {
            return;
        }
    }
    panic!("The shell never ran a typed command");
}
