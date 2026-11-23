#![cfg(windows)]
mod support;
use shellfox_runtime::*;
use shellfox_terminal::{PtyCommand, PtyConfig, PtySession};
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use support::Harness;
use windows_sys::Win32::{
    Foundation::HANDLE,
    Security::{GetTokenInformation, TokenElevation, TOKEN_QUERY},
    System::Threading::{GetCurrentProcess, GetProcessHandleCount, OpenProcessToken},
};
static SERIAL: Mutex<()> = Mutex::new(());
fn own_elevated() -> bool {
    let mut token: HANDLE = std::ptr::null_mut();
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        0
    );
    let token = unsafe { OwnedHandle::from_raw_handle(token.cast()) };
    let (mut flag, mut returned) = (0u32, 0u32);
    assert_ne!(
        unsafe {
            GetTokenInformation(
                token.as_raw_handle().cast(),
                TokenElevation,
                (&mut flag as *mut u32).cast(),
                4,
                &mut returned,
            )
        },
        0
    );
    assert_eq!(returned, 4);
    flag != 0
}
fn fixture() -> (PathBuf, PtyConfig) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .join(format!(
            "runtime-launch-{}",
            shellfox_core::Generation::new()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-runtime-fixture"));
    let mut config = PtyConfig::isolated(&root);
    config.supervisor = Some(exe.clone());
    config.command = Some(PtyCommand {
        program: exe.into_os_string(),
        args: vec!["--child-idle".into()],
    });
    (root, config)
}
fn handles() -> u32 {
    let mut count = 0;
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
        0
    );
    count
}
#[test]
fn actual_pinned_child_token_matches_real_host_inheritance_without_assuming_non_elevated() {
    let _serial = SERIAL.lock().unwrap();
    let expected = own_elevated();
    let mut host = Harness::start("--child-idle");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_IDLE_READY"));
    let facts = host.runtime.launch_privilege_facts();
    assert_eq!(facts.purpose, LaunchFactoryPurpose::OwnedPtyProgram);
    assert_eq!(facts.basis, LaunchEvidenceBasis::OriginalPinnedChildToken);
    assert_eq!(facts.stage, LaunchEvidenceStage::BeforePublicPtyReady);
    let actual = facts
        .token
        .expect("owned current-user token query must be available");
    assert_eq!(actual.elevated, expected);
    if expected {
        assert_eq!(facts.privilege, LaunchPrivilege::Elevated);
    } else if actual.ui_access || actual.integrity_rid >= 0x3000 {
        assert_eq!(facts.privilege, LaunchPrivilege::Unknown);
    } else {
        assert_eq!(facts.privilege, LaunchPrivilege::Normal);
    }
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|host| host.exited);
    assert_eq!(
        host.runtime.launch_privilege_facts(),
        facts,
        "initial facts survive retirement without describing future programs"
    );
}
#[test]
fn retained_fact_receipt_and_token_queries_hold_no_job_process_or_token_handles() {
    let _serial = SERIAL.lock().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .join(format!(
            "runtime-launch-{}",
            shellfox_core::Generation::new()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-runtime-fixture"));
    let mut config = PtyConfig::isolated(&root);
    config.supervisor = Some(exe.clone());
    config.command = Some(PtyCommand {
        program: exe.into_os_string(),
        args: vec!["--child-idle".into()],
    });
    let (binding, verifier) = shellfox_terminal::launch_privilege::new_launch_binding();
    let mut session = PtySession::spawn_bound(config, binding).unwrap();
    let receipt = session
        .take_launch_evidence()
        .expect("real factory evidence");
    assert!(verifier.matches(&receipt));
    session.shutdown().unwrap(); // real original-Job empty/pipe cleanup proof
    let facts = receipt.facts();
    let before = handles();
    for _ in 0..32 {
        assert_eq!(own_elevated(), own_elevated());
    }
    assert_eq!(
        handles(),
        before,
        "TOKEN_QUERY handles must close after each capture"
    );
    drop(receipt);
    assert_eq!(
        handles(),
        before,
        "retired fact receipt must own NO read-only Job/process handles"
    );
    assert_ne!(facts.purpose, LaunchFactoryPurpose::Unbound);
    drop(session);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn fact_receipt_can_outlive_controller_drop_without_retaining_its_os_domain() {
    let _serial = SERIAL.lock().unwrap();
    let (root, config) = fixture();
    let (binding, verifier) = shellfox_terminal::launch_privilege::new_launch_binding();
    let mut session = PtySession::spawn_bound(config, binding).unwrap();
    let receipt = session.take_launch_evidence().unwrap();
    assert!(verifier.matches(&receipt));
    drop(session);
    assert_eq!(
        receipt.facts().purpose,
        LaunchFactoryPurpose::OwnedPtyProgram
    );
    let before = handles();
    drop(receipt);
    assert_eq!(handles(), before);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn controller_drop_and_rejected_factory_purpose_do_not_transfer_handles_to_fact_receipts() {
    let _serial = SERIAL.lock().unwrap();
    let (root, config) = fixture();
    let identity = Identity {
        session_id: shellfox_core::SessionId::new(),
        tab_id: shellfox_core::TabId::new(),
        generation: shellfox_core::Generation::new(),
    };
    let mut runtime = Runtime::start_external(identity, ExternalConfig::default()).unwrap();
    let mut source = runtime.take_source().unwrap();
    let binding = source.take_launch_binding().unwrap();
    let mut session = PtySession::spawn_bound(config, binding).unwrap();
    let evidence = session.take_launch_evidence().unwrap();
    assert_eq!(
        evidence.facts().purpose,
        LaunchFactoryPurpose::OwnedPtyProgram
    );
    let before = handles();
    assert!(
        source.bind_launch_evidence(evidence).is_err(),
        "an External raw source cannot accept a PTY-purpose receipt"
    );
    assert_eq!(
        handles(),
        before,
        "failure must drop no Job/process/token handles from the fact receipt"
    );
    assert_eq!(
        runtime.launch_privilege_facts().privilege,
        LaunchPrivilege::Unknown
    );
    session.write(b"still-owned\r").unwrap();
    drop(session); // original controller performs its own exact cleanup
    source.abort(ExternalAbortReason::CancelledBeforeRawLaunch);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !runtime.external_status().unwrap().failed {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    runtime.request_external_close().unwrap();
    while !runtime.poll_external_closed().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn external_bytes_without_native_factory_evidence_stays_unknown_and_binding_is_once_only() {
    let _serial = SERIAL.lock().unwrap();
    let identity = Identity {
        session_id: shellfox_core::SessionId::new(),
        tab_id: shellfox_core::TabId::new(),
        generation: shellfox_core::Generation::new(),
    };
    let mut runtime = Runtime::start_external(identity, ExternalConfig::default()).unwrap();
    assert_eq!(
        runtime.launch_privilege_facts().privilege,
        LaunchPrivilege::Unknown
    );
    let mut source = runtime.take_source().unwrap();
    assert!(source.take_launch_binding().is_some());
    assert!(source.take_launch_binding().is_none());
    source.abort(ExternalAbortReason::CancelledBeforeRawLaunch);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !runtime.external_status().unwrap().failed {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        runtime.launch_privilege_facts().privilege,
        LaunchPrivilege::Unknown
    );
    runtime.request_external_close().unwrap();
    while !runtime.poll_external_closed().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
