use super::*;
use shellfox_protocol::{CancellationToken, Endpoint, OutboundTransport, RequestLimits};
use std::sync::{mpsc, Arc};

#[test]
fn operation_admission_is_bounded_before_native_go_runs() {
    let mut h = Harness::new();
    h.connect();
    let mut accepted = 0;
    for _ in 0..100 {
        let (status, operation) = h.operation(json!({"method":"getHomeDirectory","payload":{}}));
        if status == 0 {
            accepted += 1;
        } else {
            assert_eq!(status, BUSY);
            assert_eq!(operation, 0);
        }
    }
    assert!(
        accepted > 0 && accepted <= 3,
        "response-space reservations bound admission"
    );
    h.close();
}
#[test]
fn native_deadline_cancels_dispatched_call_without_waiting_for_go() {
    let mut h = Harness::new();
    let mobile = registry_lookup(h.handle).unwrap();
    launch(
        mobile,
        CompletionOrigin::NativeInternal,
        |mobile| async move {
            let transport = super::super::transport::NativeTransport(Arc::downgrade(&mobile));
            let limits = RequestLimits {
                deadline: Instant::now() + Duration::from_millis(100),
                cancel: CancellationToken::new(),
                max_request_bytes: 1024,
                max_response_bytes: 1024,
            };
            transport
                .send(
                    mobile.client.peer(),
                    Endpoint::Discover,
                    b"{}".to_vec(),
                    limits,
                )
                .await
                .map(|_| Value::Null)
                .map_err(|_| NETWORK)
        },
    )
    .unwrap();
    let req = h.request();
    assert_eq!(req.path, "/v1/discover");
    let event = h.event();
    assert_eq!(event["status"], NETWORK);
    assert_eq!(event["internal"], true);
    h.dispatch();
    assert!(h.mock.cancelled.contains(&(req.generation, req.id)));
    assert_eq!(h.complete(&req, PAIR), STALE);
}
struct BlockingDispatch {
    started: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
unsafe extern "C" fn blocked_request(
    context: *mut c_void,
    _: u64,
    _: u64,
    _: u64,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    _: u32,
    _: usize,
) {
    let context = unsafe { &*context.cast::<BlockingDispatch>() };
    let _ = context.started.send(());
    let _ = context.release.recv();
}
unsafe extern "C" fn no_cancel(_: *mut c_void, _: u64, _: u64, _: u64) {}
#[test]
fn close_waits_for_borrowed_dispatch_callback_before_context_can_be_freed() {
    let mut h = Harness::new();
    h.connect();
    assert_eq!(
        h.operation(json!({"method":"getHomeDirectory","payload":{}}))
            .0,
        0
    );
    let mobile = registry_lookup(h.handle).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while mobile.state.lock().unwrap().outbound.is_empty() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let handle = h.handle;
    h.handle = 0;
    let (started, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let dispatcher = thread::spawn(move || {
        let mut context = BlockingDispatch {
            started,
            release: blocked,
        };
        unsafe {
            sfm_client_dispatch(
                handle,
                Some(blocked_request),
                Some(no_cancel),
                ptr::from_mut(&mut context).cast(),
            )
        }
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let (done, closed) = mpsc::channel();
    let closer = thread::spawn(move || {
        let mut mock = MockGo::default();
        let status =
            unsafe { sfm_client_close(handle, Some(cancel), ptr::from_mut(&mut mock).cast()) };
        done.send(status).unwrap();
        mock
    });
    assert!(closed.recv_timeout(Duration::from_millis(20)).is_err());
    release.send(()).unwrap();
    assert_eq!(dispatcher.join().unwrap(), 0);
    assert_eq!(closed.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
    assert_eq!(closer.join().unwrap().cancelled.len(), 1);
    assert!(registry_lookup(handle).is_err());
}
#[test]
fn busy_native_queue_retains_output_and_failed_ack_retries_without_new_pump() {
    let mut h = Harness::new();
    h.connect();
    let terminal = crate::engine::create(80, 24).unwrap();
    assert_eq!(
        unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), terminal) },
        0
    );
    assert_eq!(
        h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
            .0,
        0
    );
    let attach = h.request();
    let attachment =
        include_bytes!("../../shellfox-client/tests/fixtures/terminal-attach-owned-success.json");
    assert_eq!(h.complete(&attach, attachment), 0);
    let event = h.event();
    reset_for_event(&h, terminal, &event);
    crate::engine::with_engine(terminal, crate::engine::pump).unwrap();
    assert_eq!(sfm_client_terminal_consumed(h.handle, terminal), 0);
    let ack = h.request();
    assert_eq!(
        h.complete(&ack, br#"{"ok":true,"value":{"acknowledged":true}}"#),
        0
    );
    h.event();
    let events: Vec<_> = (2..=129)
        .map(|sequence| {
            json!({"type":"terminal","event":{
        "type":"data","tabId":TAB,"generation":"30000000-0000-4000-8000-000000000001",
        "controlEpoch":1,"sequence":sequence,"data":"x".repeat(16384)}})
        })
        .collect();
    let response = serde_json::to_vec(&json!({"ok":true,"value":{"events":events}})).unwrap();
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut op) }, 0);
    let poll = h.request();
    assert_eq!(h.complete(&poll, &response), 0);
    let coarse = h.event();
    assert_eq!(coarse["result"]["value"]["events"], json!([]));
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut op) }, BUSY);
    let mobile = registry_lookup(h.handle).unwrap();
    {
        let state = mobile.state.lock().unwrap();
        let b = state.bindings.get(&TAB.parse().unwrap()).unwrap();
        assert_eq!(b.pending.len(), 64);
        assert_eq!(b.tickets.len(), 64);
    }
    crate::engine::with_engine(terminal, crate::engine::pump).unwrap();
    assert_eq!(sfm_client_terminal_consumed(h.handle, terminal), 0);
    let failed = h.request();
    assert_eq!(failed.body["payload"]["sequence"], 33);
    assert_eq!(
        h.complete(
            &failed,
            br#"{"ok":false,"error":{"code":"INTERNAL","message":"retry","retryable":true}}"#
        ),
        0
    );
    let failed_event = h.event();
    assert_eq!(failed_event["internal"], true);
    assert_eq!(failed_event["result"]["ok"], false);
    {
        let state = mobile.state.lock().unwrap();
        let b = state.bindings.get(&TAB.parse().unwrap()).unwrap();
        assert_eq!(
            b.cursor
                .as_ref()
                .unwrap()
                .recovery_request(b.connection)
                .unwrap()
                .after_sequence,
            Some(1)
        );
    }
    // No additional pump. Retry the same actually-consumed host cursor.
    assert_eq!(sfm_client_terminal_consumed(h.handle, terminal), 0);
    let retry = h.request();
    assert_eq!(retry.body["payload"]["sequence"], 33);
    assert_eq!(
        h.complete(&retry, br#"{"ok":true,"value":{"acknowledged":true}}"#),
        0
    );
    h.event();
    h.close();
    crate::engine::close(terminal).unwrap();
}
