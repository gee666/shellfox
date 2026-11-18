#![cfg(all(windows, feature = "test-support"))]
#[path = "support/raw_process.rs"]
mod support;
use std::time::{Duration, Instant};
use support::*;
#[test]
fn forced_stop_convention_is_distinct_from_actual_kernel_exit_and_job_proof() {
    let f = Fixture::new();
    let mut process = f.spawn(&["idle"]);
    process.request_stop().unwrap();
    let (_, _, kernel_code) = collect(&mut process);
    assert!(process.was_forcibly_stopped());
    assert!(
        kernel_code > 255,
        "Actual private termination code is not the compatibility SIGKILL convention"
    );
    assert_eq!(process.exit_code(), Some(kernel_code));
}
#[test]
fn natural_exit_seven_racing_explicit_stop_keeps_actual_status() {
    let f = Fixture::new();
    let mut process = f.spawn(&["echo"]);
    process.write(vec![], true).unwrap().wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(code) = process.poll_exit().unwrap() {
            assert_eq!(code, 7);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    process.request_stop().unwrap();
    let (_, _, code) = collect(&mut process);
    assert_eq!(code, 7);
    assert!(!process.was_forcibly_stopped());
}
