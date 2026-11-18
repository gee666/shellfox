#[cfg(windows)]
use super::support::Fixture;
#[cfg(windows)]
use shellfox_resources::reap_unconfirmed_children;
#[cfg(windows)]
use std::{
    fs,
    io::{BufRead, BufReader},
    process::Stdio,
};
use std::{io::Write, time::Duration};

#[test]
#[ignore = "synthetic subprocess entrypoint, invoked only by lifetime tests"]
fn synthetic_child() {
    assert_eq!(
        std::env::var("SHELLFOX_RESOURCE_SYNTHETIC_CHILD").unwrap(),
        "owned-fixture-only"
    );
    for key in [
        "DOTNET_STARTUP_HOOKS",
        "COMPlus_EnableDiagnostics",
        "CORECLR_PROFILER_PATH",
    ] {
        assert!(
            std::env::var_os(key).is_none(),
            "managed launch environment injection survived: {key}"
        );
    }
    println!("RESOURCE_SYNTHETIC_READY");
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(1));
}

#[cfg(windows)]
fn child(fixture: &mut Fixture) -> shellfox_resources::VerifiedChild {
    fs::copy(
        std::env::current_exe().unwrap(),
        fixture.root.join("Shellfox.ElevatedTerminal.exe"),
    )
    .unwrap();
    fixture.refresh_files();
    let bundle = fixture.open().unwrap();
    let policy = shellfox_resources::NoDynamicCodeLoad::from_trusted_review(fixture.anchor);
    let mut command = fixture
        .executable(&bundle)
        .command_with_loader_policy(&policy)
        .unwrap();
    command
        .command_mut()
        .args([
            "--exact",
            "lifetime::synthetic_child",
            "--ignored",
            "--nocapture",
        ])
        .env("SHELLFOX_RESOURCE_SYNTHETIC_CHILD", "owned-fixture-only")
        .env("DOTNET_STARTUP_HOOKS", "synthetic-do-not-load")
        .env("COMPlus_EnableDiagnostics", "1")
        .env("CORECLR_PROFILER_PATH", "synthetic-do-not-load")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().unwrap();
    let mut reader = BufReader::new(child.stdout_mut().unwrap());
    loop {
        let mut line = String::new();
        assert_ne!(
            reader.read_line(&mut line).unwrap(),
            0,
            "synthetic child exited without readiness"
        );
        if line.contains("RESOURCE_SYNTHETIC_READY") {
            break;
        }
    }
    child
}

#[cfg(windows)]
#[test]
fn process_owner_keeps_original_handles_until_os_confirmed_exit() {
    let mut fixture = Fixture::new();
    let mut child = child(&mut fixture);
    let dll = fixture.root.join("coreclr.dll");
    assert!(fs::write(&dll, b"cannot mutate while the synthetic helper runs").is_err());
    assert!(child.wait().unwrap().success());
    fs::write(dll, b"released only after wait confirmed exit").unwrap();
}

#[cfg(windows)]
#[test]
fn dropping_unconfirmed_owner_retains_handles_without_killing_child() {
    let mut fixture = Fixture::new();
    let child = child(&mut fixture);
    let dll = fixture.root.join("coreclr.dll");
    drop(child);
    assert!(fs::write(&dll, b"unconfirmed helper still owns lazy payload").is_err());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        reap_unconfirmed_children();
        if fs::write(&dll, b"released after reaper confirmed exit").is_ok() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "retained fixture did not exit"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}
