//! About a minute of real work in one X11 window: typing in a terminal, the
//! Files panel, new tabs from desktop launches, and resizes. Any UI exit fails.
//! Runs only with SHELLFOX_X11_SOAK=1 under an X server with xdotool, as in
//! linux-check.yml; see support/mod.rs for the other prerequisites.
mod support;

use std::{
    process::Command,
    time::{Duration, Instant},
};
use support::{Desktop, Host, Ui, wait_exit};

fn xdotool(args: &[&str]) -> String {
    let output = Command::new("xdotool").args(args).output().unwrap();
    assert!(output.status.success(), "xdotool {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn alive(ui: &mut Ui, step: &str) {
    if let Some(status) = ui.0.try_wait().unwrap() {
        panic!("The desktop exited during {step}: {status}");
    }
}

#[test]
fn an_x11_window_survives_a_minute_of_work() {
    if std::env::var_os("SHELLFOX_X11_SOAK").is_none_or(|value| value != "1") {
        eprintln!("Skipped: set SHELLFOX_X11_SOAK=1 under an X server with xdotool");
        return;
    }
    let desktop = Desktop::new("desktop-x11-soak-");
    let mut ui = Ui(desktop.command(&[]).spawn().unwrap());
    let host = Host(&desktop);
    desktop.wait_registered(Some(&mut ui));
    let window = xdotool(&["search", "--sync", "--name", "^Shellfox v"]);
    let window = window.lines().next().unwrap().to_owned();
    let temp = std::env::temp_dir();
    let folders = [desktop.data.parent().unwrap(), temp.as_path()];
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut round = 0;
    while Instant::now() < deadline {
        xdotool(&["windowfocus", "--sync", &window]);
        xdotool(&["type", "--delay", "20", &format!("echo soak {round}\n")]);
        alive(&mut ui, "typing");
        // Files panel open, then closed.
        for _ in 0..2 {
            xdotool(&["key", "ctrl+shift+b"]);
            std::thread::sleep(Duration::from_millis(400));
            alive(&mut ui, "the Files panel");
        }
        // A desktop launch adds a tab and switches to it.
        if round % 3 == 0 && round < 15 {
            let folder = folders[round % 2].to_str().unwrap();
            let status = desktop.command(&["add", folder]).status().unwrap();
            assert!(status.success(), "launch forwarding failed: {status}");
            alive(&mut ui, "a new tab");
        }
        let (width, height) = if round % 2 == 0 {
            ("1000", "700")
        } else {
            ("1400", "900")
        };
        xdotool(&["windowsize", &window, width, height]);
        std::thread::sleep(Duration::from_millis(500));
        alive(&mut ui, "a resize");
        round += 1;
    }
    let reports: Vec<_> = std::fs::read_dir(desktop.logs())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|name| name.to_string_lossy().starts_with("crash-"))
        .collect();
    assert!(reports.is_empty(), "crash reports: {reports:?}");
    drop(host);
    wait_exit(
        &mut ui,
        Duration::from_secs(30),
        "The desktop kept running after its host stopped",
    );
}
