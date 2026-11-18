#![cfg(all(windows, feature = "test-support"))]
#[path = "support/raw_process.rs"]
mod support;
use shellfox_terminal::raw_process::{AutomationStdinClosed, INPUT_BYTES};
use std::time::{Duration, Instant};
use support::*;
#[test]
fn known_eof_is_typed_closed_while_validation_and_actual_byte_ack_are_not() {
    let f = Fixture::new();
    let mut process = f.spawn(&["echo"]);
    let invalid = process
        .write(vec![1; INPUT_BYTES + 1], false)
        .err()
        .unwrap();
    assert!(invalid.downcast_ref::<AutomationStdinClosed>().is_none());
    let bytes = vec![0, 255, 13, 10, 128];
    let receipt = process.write(bytes.clone(), true).unwrap();
    let closed = process.write(vec![1], false).err().unwrap();
    let closed = closed.downcast_ref::<AutomationStdinClosed>().unwrap();
    assert!(closed.io_error().is_none());
    receipt.wait().unwrap();
    let (stdout, stderr, code) = collect(&mut process);
    assert_eq!(stdout, bytes);
    assert_eq!(stderr, [0, 255, 10]);
    assert_eq!(code, 7);
}
#[test]
fn actual_fast_exit_broken_pipe_receipt_preserves_native_kind_and_code() {
    let f = Fixture::new();
    let mut process = f.spawn(&["exit-code", "7"]);
    // Do not poll the root before writing: this exercises OS BrokenPipe rather
    // than the separate known-run-exited admission marker.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = match process.write(vec![42; 4096], false) {
            Ok(receipt) => receipt.wait(),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            let closed = error
                .downcast_ref::<AutomationStdinClosed>()
                .expect("Actual BrokenPipe marker");
            let native = closed
                .io_error()
                .expect("Original OS error must not become a string/known-only closure");
            assert_eq!(native.kind(), std::io::ErrorKind::BrokenPipe);
            assert!(native.raw_os_error().is_some());
            break;
        }
        assert!(Instant::now() < deadline);
    }
    let (stdout, stderr, code) = collect(&mut process);
    assert!(stdout.is_empty() && stderr.is_empty());
    assert_eq!(code, 7);
}
