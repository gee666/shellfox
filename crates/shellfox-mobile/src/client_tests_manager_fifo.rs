//! Full native C ABI admission pipeline, local HTTP completions only, no grants.
use super::*;
fn pending(h: &ManagerHarness, engine: u64) -> usize {
    let mobile = registry_lookup(h.h.handle).unwrap();
    let binding = crate::client::manager::bindings::by_engine(&mobile, engine).unwrap();
    let state = binding.state.lock().unwrap();
    state.cursor.as_ref().unwrap().pending_inputs()
}
#[test]
fn full_sixteen_input_failure_and_cancel_release_only_known_unsent_tails() {
    for cancel_front in [false, true] {
        let mut h = ManagerHarness::new();
        h.pair();
        let engine = h.bind_attach();
        h.ack(engine);
        let (first_status, first_operation) =
            h.input(engine, json!({"type":"commit","text":"front"}));
        assert_eq!(first_status, 0);
        let front = h.h.request();
        let front_id = front.body["payload"]["inputId"].clone();
        for _ in 0..15 {
            assert_eq!(
                h.input(engine, json!({"type":"commit","text":"unsent"})).0,
                0
            );
        }
        assert_eq!(pending(&h, engine), 16);
        assert_ne!(h.input(engine, json!({"type":"commit","text":"full"})).0, 0);
        if cancel_front {
            assert_eq!(sfm_client_cancel(h.h.handle, first_operation), 0);
        } else {
            h.complete(&front,json!({"version":2,"binding":h.binding,"ok":false,"error":m::Error{code:shellfox_protocol::ErrorCode::Validation,message:"Fixture refuses front".into(),retryable:false,detail:m::ErrorDetail::Application}}));
        }
        let mut cancelled = 0;
        for _ in 0..16 {
            let event = h.h.event();
            assert_eq!(event["connection"]["status"], "connected");
            if event["status"] != 0 {
                cancelled += 1;
            }
        }
        assert_eq!(cancelled, if cancel_front { 16 } else { 15 });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while pending(&h, engine) != 1 && std::time::Instant::now() < deadline {
            h.h.dispatch();
            std::thread::yield_now();
        }
        assert_eq!(pending(&h, engine), 1); // Sent front remains unknown. No acceptance or replay.
        h.h.dispatch();
        assert!(h.h.mock.requests.is_empty());
        let (status, operation) = h.input(engine, json!({"type":"commit","text":"new user input"}));
        assert_eq!(status, 0);
        assert_ne!(operation, first_operation);
        let next = h.h.request();
        assert_ne!(next.body["payload"]["inputId"], front_id);
        assert_eq!(
            next.body["payload"]["input"]["event"]["text"],
            "new user input"
        );
        let id = next.body["payload"]["inputId"].clone();
        h.complete(
            &next,
            h.bound(json!({"inputId":id,"kind":"key","acceptedBytes":14,"disposition":"accepted"})),
        );
        assert_eq!(h.h.event()["status"], 0);
        assert_eq!(pending(&h, engine), 1);
        assert_eq!(sfm_manager_v2_unbind_terminal(h.h.handle, engine), 0);
        crate::engine::close(engine).unwrap();
    }
}
