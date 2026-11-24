//! Launches the real desktop binary with its host process and waits until a
//! window has attached. See support/mod.rs for the prerequisites.
mod support;

use std::time::{Duration, Instant};
use support::{Desktop, Host, Ui, wait_exit};

#[test]
fn desktop_window_attaches_to_its_own_host_and_quits_with_it() {
    let desktop = Desktop::new("desktop-launch-smoke-");
    let mut ui = Ui(desktop.command(&[]).spawn().unwrap());
    let host = Host(&desktop);
    desktop.wait_registered(Some(&mut ui));
    // Startup errors that surface after the first window still end the process.
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        ui.0.try_wait().unwrap().is_none(),
        "The desktop exited after attaching"
    );
    drop(host);
    // An orderly stop of its own host quits the desktop quietly.
    wait_exit(
        &mut ui,
        Duration::from_secs(30),
        "The desktop kept running after its host stopped",
    );
    let log = std::fs::read_to_string(desktop.logs().join("ui.log")).unwrap();
    assert!(log.contains("--- Shellfox ui "), "{log}");
}

#[test]
fn a_crashed_ui_leaves_a_report_and_its_host_relaunches_it() {
    let desktop = Desktop::new("desktop-crash-relaunch-");
    let mut ui = Ui(desktop
        .command(&[])
        .env(shellfox_service::crash_report::TEST_PANIC_ENV, "1")
        .spawn()
        .unwrap());
    let host = Host(&desktop);
    wait_exit(
        &mut ui,
        Duration::from_secs(90),
        "The forced panic did not end the UI",
    );
    assert!(!ui.0.wait().unwrap().success());
    // The relaunched UI attaches to the same host.
    desktop.wait_registered(None);
    let reports: Vec<_> = std::fs::read_dir(desktop.logs())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("crash-"))
        .collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].ends_with("-ui.txt"));
    let report = std::fs::read_to_string(desktop.logs().join(&reports[0])).unwrap();
    for expected in [
        "Shellfox ui panic",
        "Forced test panic",
        "Backtrace:",
        "Last log lines",
    ] {
        assert!(report.contains(expected), "{expected} missing:\n{report}");
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let host_log = std::fs::read_to_string(desktop.logs().join("host.log")).unwrap_or_default();
        if host_log.contains("UI ended without quitting: Relaunch") {
            break;
        }
        assert!(Instant::now() < deadline, "{host_log}");
        std::thread::sleep(Duration::from_millis(200));
    }
    // The relaunched UI is the host's child: it quits with the host, and
    // then releases the scratch directory.
    drop(host);
    let deadline = Instant::now() + Duration::from_secs(30);
    while desktop.registered() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_secs(2));
}
