//! About a minute of real work in one X11 window: commands typed in a
//! terminal, the Files panel, new tabs from desktop launches, and resizes.
//! Every typed command must run, and any UI exit fails.
//! Runs only with SHELLFOX_X11_SOAK=1 under an X server with xdotool, as in
//! linux-check.yml; see support/mod.rs for the other prerequisites.
mod support;

use std::time::{Duration, Instant};
use support::{
    Desktop, Host, Ui, wait_exit,
    x11::{type_line, wait_for_shell, wait_until, window, xdotool},
};

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
    let window = window();
    let scratch = desktop.data.parent().unwrap().to_path_buf();
    let temp = std::env::temp_dir();
    let folders = [scratch.as_path(), temp.as_path()];
    // A fresh data dir opens with no sessions, so there is no shell to type
    // into until a launch adds the first tab.
    let status = desktop
        .command(&["add", scratch.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success(), "launch forwarding failed: {status}");
    wait_for_shell(&window, &scratch, &mut ui);
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut round = 0;
    while Instant::now() < deadline {
        // The full line must reach the shell: a lost key changes the output.
        let output = scratch.join(format!("soak-{round}"));
        let line = format!("soak {round} the quick brown fox");
        type_line(&window, &format!("echo {line} > '{}'", output.display()));
        let ran = wait_until(&mut ui, Duration::from_secs(10), || {
            std::fs::read_to_string(&output).is_ok_and(|text| text == format!("{line}\n"))
        });
        let text = std::fs::read_to_string(&output);
        assert!(ran, "round {round}: the typed command wrote {text:?}");
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
            // The next keys must reach the new tab's shell, not race its start.
            wait_for_shell(&window, &scratch, &mut ui);
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
