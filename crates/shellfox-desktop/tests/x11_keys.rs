//! Tab and Shift+Tab typed into a focused X11 terminal reach the program in it,
//! in legacy encoding and with pi-tui's kitty keyboard flags (CSI > 7 u), and
//! the app's focus traversal never takes them. Runs only with
//! SHELLFOX_X11_KEYS=1 under an X server with xdotool and python3, as in
//! linux-check.yml; see support/mod.rs for the other prerequisites.
mod support;

use std::{
    path::Path,
    time::{Duration, Instant},
};
use support::{
    Desktop, Host, Ui, wait_exit,
    x11::{type_line, wait_for_shell, window, xdotool},
};

/// Raw-mode reader: optionally pushes kitty flags, writes "ready", then
/// appends every read as hex until it reads "q".
const LOGGER: &str = r#"
import os, sys, termios, tty
log, flags = sys.argv[1], sys.argv[2]
fd = sys.stdin.fileno(); old = termios.tcgetattr(fd); tty.setraw(fd)
if flags != "0": os.write(1, f"\x1b[>{flags}u".encode())
with open(log, "a") as f: f.write("ready\n")
try:
    while True:
        data = os.read(fd, 256)
        if data == b"q": break
        with open(log, "a") as f: f.write(data.hex() + "\n")
finally:
    if flags != "0": os.write(1, b"\x1b[<u")
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
"#;

fn wait_for(log: &Path, ui: &mut Ui, done: impl Fn(&str) -> bool, what: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if done(&text) {
            return text;
        }
        if let Some(status) = ui.0.try_wait().unwrap() {
            panic!("The desktop exited while waiting for {what}: {status}");
        }
        assert!(Instant::now() < deadline, "{what}; log so far: {text:?}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn tab_and_shift_tab_reach_the_focused_x11_terminal() {
    if std::env::var_os("SHELLFOX_X11_KEYS").is_none_or(|value| value != "1") {
        eprintln!("Skipped: set SHELLFOX_X11_KEYS=1 under an X server with xdotool and python3");
        return;
    }
    let desktop = Desktop::new("desktop-x11-keys-");
    let folder = desktop.data.parent().unwrap().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("keylog.py"), LOGGER).unwrap();
    let mut ui = Ui(desktop.command(&[]).spawn().unwrap());
    let host = Host(&desktop);
    desktop.wait_registered(Some(&mut ui));
    desktop.forward(&["add", folder.to_str().unwrap()]);
    let window = window();
    wait_for_shell(&window, &folder, &mut ui);
    // (kitty flags, Tab bytes, Shift+Tab bytes) as hex.
    for (flags, tab, shift_tab) in [("0", "09", "1b5b5a"), ("7", "09", "1b5b393b3275")] {
        let log = folder.join(format!("keys-{flags}.log"));
        type_line(
            &window,
            &format!("python3 keylog.py keys-{flags}.log {flags}"),
        );
        wait_for(
            &log,
            &mut ui,
            |text| text.starts_with("ready\n"),
            "the logger",
        );
        xdotool(&["key", "--delay", "150", "Tab", "shift+Tab", "q"]);
        let expected = format!("{tab}{shift_tab}");
        let received = |text: &str| text.trim_start_matches("ready\n").replace('\n', "");
        let text = wait_for(
            &log,
            &mut ui,
            |text| received(text).len() >= expected.len(),
            "both keys",
        );
        assert_eq!(received(&text), expected, "kitty flags {flags}");
    }
    drop(host);
    wait_exit(
        &mut ui,
        Duration::from_secs(30),
        "The desktop kept running after its host stopped",
    );
}
