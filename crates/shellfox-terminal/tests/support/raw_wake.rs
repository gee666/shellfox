//! Output and exit wakes let an owner block between raw process events.
use super::{support::Fixture, wakes::Wakes};
use shellfox_terminal::raw_process::{RawOutput, RawProcess, RawStream};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Reads to exit, checking state only after a wake. A missing wake times out.
fn drain_on_wakes(process: &mut RawProcess, wakes: &Wakes) -> (Vec<u8>, i32) {
    let mut stdout = Vec::new();
    let mut eof = 0;
    loop {
        let seen = wakes.count();
        while let Some(output) = process.try_output() {
            match output {
                RawOutput::Data {
                    stream: RawStream::Stdout,
                    bytes,
                } => stdout.extend(bytes),
                RawOutput::Data { .. } => {}
                RawOutput::Eof(_) => eof += 1,
                RawOutput::Failed(stream) => panic!("raw pipe failed: {stream:?}"),
            }
        }
        if let Some(code) = process.poll_exit().unwrap().filter(|_| eof == 2) {
            process.close().unwrap();
            return (stdout, code);
        }
        wakes.wait_past(seen);
    }
}

#[test]
fn output_and_tree_exit_wake_the_owner() {
    let f = Fixture::new();
    let marker = f.directory.join("descendant");
    // The root exits first; its descendant keeps the pipes and the tree open
    // until poll_exit stops it, so only the empty-tree wake can finish this.
    let mut process = f.spawn(&["branch", marker.to_str().unwrap()]);
    process.release_stdin();
    let wakes = Arc::new(Wakes::default());
    process.set_output_wake(wakes.wake()).unwrap();
    let (stdout, code) = drain_on_wakes(&mut process, &wakes);
    assert_eq!(stdout, b"ROOT_READY\n");
    assert_eq!(code, 0);
    assert!(!marker.exists(), "the descendant outlived the raw process");
}

#[test]
fn a_late_wake_fires_for_output_already_queued() {
    let f = Fixture::new();
    let mut process = f.spawn(&["echo"]);
    process
        .write(b"queued".to_vec(), true)
        .unwrap()
        .wait()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while process.poll_exit().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the echo fixture did not exit");
        std::thread::sleep(Duration::from_millis(5));
    }
    let wakes = Arc::new(Wakes::default());
    process.set_output_wake(wakes.wake()).unwrap();
    assert!(
        wakes.count() >= 1,
        "registration must wake for queued output"
    );
    let (stdout, code) = drain_on_wakes(&mut process, &wakes);
    assert_eq!(stdout, b"queued");
    assert_eq!(code, 7);
}
