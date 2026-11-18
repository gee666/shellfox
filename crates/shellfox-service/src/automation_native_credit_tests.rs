use super::*;

#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn another_client_stop_releases_capture_credit_but_preserves_final_source() {
    let mut f = Fixture::new();
    let input = f.input(&["flood"], false);
    let run = f.start(input, 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while f.manager.runs[&run.id].bytes < AUTOMATION_CREDIT {
        f.manager.pump().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    f.manager.begin_stop(AutomationStop { id: run.id }).unwrap();
    assert!(!f.manager.runs[&run.id].capturing);
    while !f.manager.is_stopped(run.id).unwrap() {
        f.manager.pump().unwrap();
        assert!(
            Instant::now() < deadline,
            "Unacknowledged original pipe credit blocked stop"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(f.manager.active_processes(), 0);
    let final_status = f
        .manager
        .read(
            AutomationRead {
                id: run.id,
                after: 0,
            },
            1,
        )
        .unwrap();
    assert!(final_status.frames.is_empty());
    assert_eq!(final_status.run.state, AutomationState::Exited);
    assert!(final_status.run.exit_code.is_none());
    assert_eq!(final_status.run.signal.as_deref(), Some("SIGKILL"));
    let events = f.feeds[0].worker.take().unwrap().join().unwrap();
    let mut sequence = 0;
    let mut eof = 0;
    let mut exit = false;
    for event in events {
        match event {
            AutomationViewEvent::Data { sequence: next, .. } => {
                assert!(!exit);
                assert_eq!(next, sequence + 1);
                sequence = next;
            }
            AutomationViewEvent::Eof { after_sequence, .. } => {
                assert_eq!(after_sequence, sequence);
                eof += 1;
            }
            AutomationViewEvent::Exit { after_sequence, .. } => {
                assert_eq!(after_sequence, sequence);
                assert_eq!(eof, 2);
                exit = true;
            }
        }
    }
    assert!(exit);
}
