use super::*;
fn write(service: &mut Service, id: Uuid, bytes: &[u8], eof: bool) {
    service
        .automation
        .runs
        .begin_write(
            AutomationWrite {
                id,
                data: shellfox_protocol::encode_chunk(bytes).unwrap(),
                eof,
            },
            9,
        )
        .unwrap()
        .wait()
        .unwrap();
}
fn consume(service: &mut Service, id: Uuid, after: &mut u64, expected: usize) {
    let mut count = 0;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        service.pump();
        let output = service
            .automation
            .runs
            .read(AutomationRead { id, after: *after }, 9)
            .unwrap();
        for frame in output.frames {
            count += shellfox_protocol::decode_chunk(&frame.data).unwrap().len();
            *after = frame.sequence;
        }
        let identity = service.automation.relays[&id].feed.identity();
        if count >= expected
            && service.terminals[&identity.tab_id]
                .runtime
                .external_status()
                .unwrap()
                .source_sequence
                .0
                >= *after
        {
            service
                .automation
                .runs
                .read(AutomationRead { id, after: *after }, 9)
                .unwrap();
            return;
        }
        assert!(
            Instant::now() < deadline,
            "actual raw display did not reach its single sink"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
#[ignore = "explicit owned raw fixture only; no installed data or auth"]
fn actual_external_output_is_unbound_before_show_then_idle_and_exit_touch_without_replay() {
    let (scratch, mut owned, helper) = service();
    let service = &mut owned.0;
    let mut spec = input(&helper, scratch.path(), vec!["echo".into()]);
    spec.detached = false;
    let run = service.start_automation(spec, 9).unwrap();
    let identity = service.automation.relays[&run.id].feed.identity();
    let mut after = 0;
    write(service, run.id, b"before\n", false);
    consume(service, run.id, &mut after, 7);
    assert!(service.host.snapshot().unwrap().sessions.is_empty());
    assert_eq!(service.activity.persistence.pending_count(), 0);
    assert!(service.activity.owner.live_output_sink(identity).is_none());
    show(service, run.id);
    let sink = service.activity.owner.live_output_sink(identity).unwrap();
    let initial = service.host.snapshot().unwrap().sessions[0]
        .updated_at
        .clone();
    std::thread::sleep(Duration::from_millis(15));
    let idle = b"\x1b[?25h".repeat(1024); // Known idle setter; large enough to bypass stdio line buffering.
    write(service, run.id, &idle, false);
    consume(service, run.id, &mut after, idle.len());
    assert!(sink.last_sequence().is_some_and(|sequence| sequence > 0));
    assert!(sink.last_meaningful_output_ms().is_none());
    service.flush_activity(true).unwrap();
    assert_ne!(
        service.host.snapshot().unwrap().sessions[0].updated_at,
        initial
    );
    show(service, run.id);
    assert!(std::sync::Arc::ptr_eq(
        &sink,
        &service.activity.owner.live_output_sink(identity).unwrap()
    ));
    write(service, run.id, &[], true);
    finish(service, run.id);
    service.pump();
    service.flush_activity(true).unwrap();
    let exited = service.host.snapshot().unwrap().sessions[0]
        .updated_at
        .clone();
    std::thread::sleep(Duration::from_millis(15));
    service.activity.next_poll_ms = 0;
    service.pump();
    service.flush_activity(true).unwrap();
    assert_eq!(
        service.host.snapshot().unwrap().sessions[0].updated_at,
        exited,
        "silence/poll replay touched ended source"
    );
    service.close_tab(identity.tab_id).unwrap();
    assert!(service.activity.owner.live_output_sink(identity).is_none());
    service.shutdown().unwrap();
}
#[test]
#[ignore = "explicit owned raw fixture only; no installed data or auth"]
fn failed_host_publication_leaves_no_program_watch_metadata_or_touch() {
    let (scratch, mut owned, helper) = service();
    let service = &mut owned.0;
    let mut spec = input(&helper, scratch.path(), vec!["echo".into()]);
    spec.detached = false;
    spec.title = Some("invalid\ntitle".into());
    let run = service.start_automation(spec, 9).unwrap();
    let identity = service.automation.relays[&run.id].feed.identity();
    let mut router = DesktopLaunchRouter::new();
    let _registered = router.register(router.owner(), service).unwrap();
    assert!(service
        .show_automation(AutomationShow { id: run.id }, &mut router)
        .is_err());
    let mut after = 0;
    write(service, run.id, b"after rejected publication\n", false);
    consume(service, run.id, &mut after, 27);
    assert!(service.host.snapshot().unwrap().sessions.is_empty());
    assert!(service.activity.owner.live_output_sink(identity).is_none());
    assert_eq!(service.activity.persistence.pending_count(), 0);
    service.shutdown().unwrap();
}
