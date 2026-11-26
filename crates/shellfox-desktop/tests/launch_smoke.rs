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
    // A clean exit removes its run directory.
    let runs: Vec<_> = std::fs::read_dir(desktop.data.with_file_name("host.desktop"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|name| name.to_string_lossy().starts_with("run-"))
        .collect();
    assert!(runs.is_empty(), "{runs:?}");
}

/// As with 0.9's single-instance lock, of two launches a few milliseconds
/// apart one opens the window and the other waits for it, hands its folder
/// over and exits. Either may win.
#[test]
fn a_second_launch_within_milliseconds_forwards_to_the_first() {
    let desktop = Desktop::new("desktop-double-launch-");
    let folder = desktop.data.parent().unwrap().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    let folder = folder.to_str().unwrap().to_owned();
    let mut launches = [
        Ui(desktop.command(&[]).spawn().unwrap()),
        Ui(desktop.command(&["add", &folder]).spawn().unwrap()),
    ];
    let host = Host(&desktop);
    let deadline = Instant::now() + Duration::from_secs(90);
    let forwarded = loop {
        if let Some(index) = launches
            .iter_mut()
            .position(|ui| ui.0.try_wait().unwrap().is_some())
        {
            break index;
        }
        assert!(Instant::now() < deadline, "Neither launch forwarded");
        std::thread::sleep(Duration::from_millis(100));
    };
    let status = launches[forwarded].0.wait().unwrap();
    assert!(
        status.success(),
        "The forwarding launch exited with {status}"
    );
    let first = &mut launches[1 - forwarded];
    desktop.wait_registered(Some(first));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !desktop.session_folders().contains(&folder) {
        assert!(
            Instant::now() < deadline,
            "The forwarded folder never opened: {:?}",
            desktop.session_folders()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(desktop.hosts_started(), 1, "A second host was started");
    assert!(
        first.0.try_wait().unwrap().is_none(),
        "The first desktop exited"
    );
    drop(host);
    wait_exit(
        first,
        Duration::from_secs(30),
        "The desktop kept running after its host stopped",
    );
}

#[test]
fn a_crashed_ui_leaves_a_report_and_its_host_relaunches_it() {
    crash_and_relaunch(
        "1",
        &[
            "Shellfox ui panic",
            "Forced test panic",
            "Backtrace:",
            "Last log lines",
        ],
    );
}

/// A native crash or signal leaves no report from the UI; its host writes one.
#[test]
fn an_aborted_ui_gets_a_report_from_its_host_and_is_relaunched() {
    crash_and_relaunch(
        "abort",
        &[
            "Shellfox ui unexpected exit",
            "Reported by: host",
            "Last log lines",
        ],
    );
}

fn crash_and_relaunch(mode: &str, expected: &[&str]) {
    let desktop = Desktop::new(&format!("desktop-crash-{mode}-"));
    let mut ui = Ui(desktop
        .command(&[])
        .env(shellfox_service::crash_report::TEST_PANIC_ENV, mode)
        .spawn()
        .unwrap());
    let host = Host(&desktop);
    wait_exit(
        &mut ui,
        Duration::from_secs(90),
        "The forced crash did not end the UI",
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
    for expected in expected {
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

#[test]
fn gpu_failures_are_relaunched_three_times_then_given_up() {
    let desktop = Desktop::new("desktop-gpu-relaunch-");
    let mut ui = Ui(desktop
        .command(&[])
        .env(shellfox_service::crash_report::TEST_PANIC_ENV, "gpu")
        .spawn()
        .unwrap());
    let _host = Host(&desktop);
    wait_exit(
        &mut ui,
        Duration::from_secs(90),
        "The forced GPU failure did not end the UI",
    );
    // Each UI panics 5 s after its window opens; waits are 1, 3 and 10 s.
    let host_log = desktop.logs().join("host.log");
    let deadline = Instant::now() + Duration::from_secs(180);
    let log = loop {
        let log = std::fs::read_to_string(&host_log).unwrap_or_default();
        if log.contains("crashed too often") {
            break log;
        }
        assert!(Instant::now() < deadline, "{log}");
        std::thread::sleep(Duration::from_millis(500));
    };
    assert_eq!(
        log.matches("Shellfox relaunched UI ended").count(),
        3,
        "{log}"
    );
    let reports: Vec<_> = std::fs::read_dir(desktop.logs())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("crash-")
        })
        .collect();
    assert!(reports.len() >= 2, "{reports:?}");
    assert!(
        reports
            .iter()
            .all(|report| shellfox_service::crash_report::is_gpu_failure(report)),
        "{reports:?}"
    );
}
