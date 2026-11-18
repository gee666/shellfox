use super::*;

#[test]
#[ignore = "explicit owned raw fixture only; set SHELLFOX_RAW_TEST_HELPER"]
fn failed_raw_spawn_aborts_ready_parser_without_fake_finish_or_new_metadata() {
    let (scratch, mut owned, helper) = service();
    let service = &mut owned.0;
    let mut spec = input(&helper, scratch.path(), vec!["describe".into()]);
    spec.executable = scratch
        .path()
        .join("missing-owned-target.exe")
        .to_str()
        .unwrap()
        .into();
    let id = spec.request_id;
    assert!(service.start_automation(spec, 9).is_err());
    let relay = &service.automation.relays[&id];
    assert!(relay.failed_start && relay.feed.failure().is_some());
    assert!(
        !service.terminals[&relay.feed.identity().tab_id]
            .runtime
            .external_status()
            .unwrap()
            .finished
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !service.automation.relays.is_empty() {
        service.pump();
        assert!(
            Instant::now() < deadline,
            "cancelled original parser owner was lost"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(service.terminals.is_empty());
    assert_eq!(service.automation.runs.active_processes(), 0);
    assert!(service.host.snapshot().unwrap().sessions.is_empty());
    assert_eq!(
        service.shutdown().unwrap().presentation,
        PresentationShutdownState::NoKnownFailure
    );
}
#[test]
#[ignore = "explicit owned raw fixture only; set SHELLFOX_RAW_TEST_HELPER"]
fn failed_backpressured_parser_still_confirms_raw_tree_and_reports_retained_warning() {
    let (scratch, mut owned, helper) = service();
    let service = &mut owned.0;
    let marker = scratch.path().join("must-not-exist");
    let mut spec = input(
        &helper,
        scratch.path(),
        vec!["flood-branch".into(), marker.to_str().unwrap().into()],
    );
    spec.detached = false;
    let run = service.start_automation(spec, 9).unwrap();
    let identity = service.automation.relays[&run.id].feed.identity();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        service.pump();
        let output = service
            .automation
            .runs
            .read(
                AutomationRead {
                    id: run.id,
                    after: 0,
                },
                9,
            )
            .unwrap();
        if output.frames.len() >= 16 {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    // Force actual parser closure while its separately owned raw process lives.
    service
        .automation
        .relays
        .get_mut(&run.id)
        .unwrap()
        .feed
        .disconnect_for_test();
    let joined_deadline = Instant::now() + Duration::from_secs(5);
    while !service.terminals[&identity.tab_id]
        .runtime
        .external_status()
        .unwrap()
        .failed
    {
        service.drain(identity.tab_id).unwrap();
        assert!(
            Instant::now() < joined_deadline,
            "actual sender disconnect was not processed"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    service.terminals[&identity.tab_id]
        .runtime
        .request_external_close()
        .unwrap();
    while !service
        .terminals
        .get_mut(&identity.tab_id)
        .unwrap()
        .runtime
        .poll_external_closed()
        .unwrap()
    {
        assert!(
            Instant::now() < joined_deadline,
            "original parser join was not confirmed"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        !service.terminals[&identity.tab_id]
            .runtime
            .external_status()
            .unwrap()
            .finished
    );
    assert!(!service.automation.runs.is_stopped(run.id).unwrap());
    // Release the caller pipe, not the raw owner, and retain the exact rejected
    // next source chunk. No synthetic EOF/Finish/ACK is introduced.
    service.automation.runs.release_owner(9);
    service.automation.runs.pump().unwrap();
    assert!(service
        .automation
        .relays
        .get_mut(&run.id)
        .unwrap()
        .feed
        .pump()
        .is_err());
    let failure = service.automation.relays[&run.id].feed.failure().unwrap();
    assert!(failure.pending_bytes() > 0);
    service
        .begin_automation_stop(AutomationStop { id: run.id })
        .unwrap();
    let outcome = service.shutdown().unwrap();
    let PresentationShutdownState::FailedSources(warnings) = outcome.presentation else {
        panic!("failed final rendering was not reported");
    };
    assert_eq!(warnings.failures().len(), 1);
    assert!(warnings.failures()[0].raw_cleanup_confirmed());
    assert!(warnings.failures()[0].failure().pending_bytes() > 0);
    assert_eq!(warnings.failures()[0].failure().identity(), Some(identity));
    assert!(!format!("{warnings:?}").contains("private fixture value"));
    assert!(service.terminals.is_empty());
    std::thread::sleep(Duration::from_secs(5));
    assert!(
        !marker.exists(),
        "owned descendant survived actual Job cleanup"
    );
}
