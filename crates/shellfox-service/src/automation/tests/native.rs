use super::*;
mod credits;
mod exit_status;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    thread::JoinHandle,
};

struct Fixture {
    manager: AutomationRuns,
    directory: PathBuf,
    helper: PathBuf,
    feeds: Vec<Feed>,
}
struct Feed {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Vec<AutomationViewEvent>>>,
}
impl Fixture {
    fn new() -> Self {
        let helper = PathBuf::from(std::env::var_os("SHELLFOX_RAW_TEST_HELPER").expect(
            "Build shellfox-raw-test-helper with test-support and pass its absolute path.",
        ));
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tmp")
            .join(format!("automation-owned-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        Self {
            manager: AutomationRuns::new(helper.clone()),
            directory,
            helper,
            feeds: vec![],
        }
    }
    fn input(&self, args: &[&str], detached: bool) -> AutomationStart {
        AutomationStart {
            request_id: Uuid::new_v4(),
            executable: self.helper.to_str().unwrap().into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            cwd: self.directory.to_str().unwrap().into(),
            env: BTreeMap::new(),
            title: None,
            detached,
        }
    }
    fn start(&mut self, input: AutomationStart, owner: u64) -> AutomationRun {
        assert!(self.manager.reserve_start(&input, owner).unwrap().is_none());
        let source = self.manager.take_source(input.request_id).unwrap();
        assert!(self.manager.take_source(input.request_id).is_err());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut events = vec![];
            while !stopping.load(Ordering::Acquire) {
                while let Some(event) = source.try_recv() {
                    let exit = matches!(event, AutomationViewEvent::Exit { .. });
                    events.push(event);
                    if exit {
                        return events;
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            events
        });
        self.feeds.push(Feed {
            stop,
            worker: Some(worker),
        });
        self.manager.start(input, owner).unwrap()
    }
    fn collect(&mut self, id: Uuid, owner: u64) -> (Vec<u8>, Vec<u8>, i64) {
        let mut stdout = vec![];
        let mut stderr = vec![];
        let mut after = 0;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let output = self
                .manager
                .read(AutomationRead { id, after }, owner)
                .unwrap();
            for frame in &output.frames {
                let bytes = shellfox_protocol::decode_chunk(&frame.data).unwrap();
                if frame.stream == AutomationStream::Stdout {
                    stdout.extend(bytes);
                } else {
                    stderr.extend(bytes);
                }
                after = frame.sequence;
            }
            if output.frames.is_empty() && output.run.state == AutomationState::Exited {
                return (stdout, stderr, output.run.exit_code.unwrap());
            }
            assert!(Instant::now() < deadline, "Owned automation did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.manager.shutdown().unwrap();
        for feed in &mut self.feeds {
            feed.stop.store(true, Ordering::Release);
            if let Some(worker) = feed.worker.take() {
                worker.join().unwrap();
            }
        }
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn binary_pipe_owner_idempotency_cursor_and_source_order() {
    let mut f = Fixture::new();
    let input = f.input(&["echo"], false);
    let run = f.start(input.clone(), 1);
    assert_eq!(f.manager.start(input.clone(), 1).unwrap().pid, run.pid);
    assert!(f.manager.start(input.clone(), 2).is_err());
    let mut changed = input;
    changed.args.push("literal".into());
    assert!(f.manager.start(changed, 1).is_err());
    assert!(f
        .manager
        .read(
            AutomationRead {
                id: run.id,
                after: 0
            },
            2
        )
        .is_err());
    assert!(f
        .manager
        .begin_write(
            AutomationWrite {
                id: run.id,
                data: String::new(),
                eof: true
            },
            2
        )
        .is_err());
    let bytes = [0, 255, 10, 128];
    f.manager
        .begin_write(
            AutomationWrite {
                id: run.id,
                data: shellfox_protocol::encode_chunk(&bytes).unwrap(),
                eof: true,
            },
            1,
        )
        .unwrap()
        .wait()
        .unwrap();
    let (stdout, stderr, code) = f.collect(run.id, 1);
    assert_eq!(stdout, bytes);
    assert_eq!(stderr, [0, 255, 10]);
    assert_eq!(code, 7);
    f.manager.begin_stop(AutomationStop { id: run.id }).unwrap();
    assert_eq!(f.manager.runs[&run.id].dto.exit_code, Some(7));
    assert!(f.manager.runs[&run.id].dto.signal.is_none());
    let ended = f
        .manager
        .begin_write(
            AutomationWrite {
                id: run.id,
                data: String::new(),
                eof: true,
            },
            1,
        )
        .err()
        .unwrap();
    assert!(ended.downcast_ref::<AutomationStdinClosed>().is_some());
    let unauthorized = f
        .manager
        .begin_write(
            AutomationWrite {
                id: run.id,
                data: String::new(),
                eof: true,
            },
            2,
        )
        .err()
        .unwrap();
    assert!(unauthorized
        .downcast_ref::<AutomationStdinClosed>()
        .is_none());
    assert!(f
        .manager
        .read(
            AutomationRead {
                id: run.id,
                after: 0
            },
            1
        )
        .is_err());
    let events = f.feeds[0].worker.take().unwrap().join().unwrap();
    let mut sequence = 0;
    let mut eof = 0;
    let mut source_out = vec![];
    let mut source_err = vec![];
    for event in events {
        match event {
            AutomationViewEvent::Data {
                sequence: next,
                stream,
                bytes,
            } => {
                assert_eq!(next, sequence + 1);
                sequence = next;
                if stream == AutomationStream::Stdout {
                    source_out.extend(bytes);
                } else {
                    source_err.extend(bytes);
                }
            }
            AutomationViewEvent::Eof { after_sequence, .. } => {
                assert_eq!(after_sequence, sequence);
                eof += 1;
            }
            AutomationViewEvent::Exit {
                after_sequence,
                code,
            } => {
                assert_eq!(after_sequence, sequence);
                assert_eq!(eof, 2);
                assert_eq!(code, 7);
            }
        }
    }
    assert_eq!(source_out, stdout);
    assert_eq!(source_err, stderr);
}
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn retained_credit_replays_exact_frames_and_large_output() {
    let mut f = Fixture::new();
    let input = f.input(&["flood"], false);
    let run = f.start(input, 1);
    std::thread::sleep(Duration::from_millis(100));
    f.manager.pump().unwrap();
    let first = f
        .manager
        .read(
            AutomationRead {
                id: run.id,
                after: 0,
            },
            1,
        )
        .unwrap();
    let replay = f
        .manager
        .read(
            AutomationRead {
                id: run.id,
                after: 0,
            },
            1,
        )
        .unwrap();
    assert_eq!(first.frames, replay.frames);
    assert!(f.manager.runs[&run.id].bytes <= AUTOMATION_CREDIT);
    let (stdout, stderr, code) = f.collect(run.id, 1);
    assert_eq!(stdout.len(), 3 * 1024 * 1024);
    assert_eq!(stderr.len(), 1024 * 1024);
    assert_eq!(code, 0);
}
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn disconnect_detached_and_lease_expiry_are_eof_not_kill() {
    let mut f = Fixture::new();
    let input = f.input(&["eof"], false);
    let run = f.start(input, 1);
    f.manager.release_owner(1);
    f.manager.release_owner(1);
    assert!(f
        .manager
        .read(
            AutomationRead {
                id: run.id,
                after: 0
            },
            1
        )
        .is_err());
    let input = f.input(&["eof"], true);
    let detached = f.start(input, 2);
    let input = f.input(&["eof"], false);
    let expired = f.start(input, 3);
    f.manager.runs.get_mut(&expired.id).unwrap().last_read =
        Instant::now() - Duration::from_secs(31);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let runs = f.manager.list().unwrap();
        if runs.0.iter().all(|r| r.state == AutomationState::Exited) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    for id in [run.id, detached.id, expired.id] {
        assert_eq!(f.manager.runs[&id].dto.exit_code, Some(0));
    }
    assert_eq!(f.manager.active_processes(), 0);
}
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn explicit_stop_and_shutdown_have_actual_raw_cleanup_proof() {
    let mut f = Fixture::new();
    let input = f.input(&["idle"], false);
    let run = f.start(input, 1);
    f.manager.begin_stop(AutomationStop { id: run.id }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f.manager.is_stopped(run.id).unwrap() {
        f.manager.pump().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        f.manager
            .stop(AutomationStop { id: run.id })
            .unwrap()
            .stopped
    );
    let input = f.input(&["idle"], false);
    f.start(input, 2);
    f.manager.shutdown().unwrap();
    assert_eq!(f.manager.active_processes(), 0);
}
