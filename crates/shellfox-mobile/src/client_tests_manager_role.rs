//! Manager C ABI role/lifetime fences using local HTTP-completion fixtures.
use super::*;
fn progress(engine: u64) -> (u64, u64, u32, u32) {
    let mut value = crate::SfmProgress::default();
    assert_eq!(
        unsafe { crate::sfm_terminal_progress(engine, &mut value) },
        0
    );
    (
        value.accepted_sequence,
        value.consumed_sequence,
        value.queued_bytes,
        value.queued_chunks,
    )
}
#[test]
fn external_reset_and_submit_cannot_forge_manager_data_consumption() {
    let mut h = ManagerHarness::new();
    h.pair();
    let value = attachment();
    let m::Attachment::Attached(ref attached) = value else {
        panic!()
    };
    let delivery = attached.delivery.clone();
    let engine = h.bind_attach_value(value);
    h.ack(engine);
    let before_frame = crate::engine::with_engine(engine, |owned| {
        owned.terminal.render().map_err(|_| crate::Status::Engine)
    })
    .unwrap();
    let mut op = 0;
    assert_eq!(unsafe { sfm_manager_v2_poll(h.h.handle, &mut op) }, 0);
    let request = h.h.request();
    let bytes = b"255;0;0mHOST-DATA";
    let response = m::PollResponse {
        events: vec![m::HostEvent::Terminal {
            event: m::Event::Data {
                delivery,
                sequence: 1,
                encoding: m::Encoding::Base64,
                data: shellfox_protocol::encode_chunk(bytes).unwrap(),
            },
        }],
    };
    h.complete(&request, h.bound(serde_json::to_value(response).unwrap()));
    assert_eq!(h.h.event()["status"], 0);
    let before = progress(engine);
    assert!(before.0 > before.1 && before.3 > 0);
    assert_eq!(
        crate::sfm_terminal_reset(engine, 80, 24),
        crate::Status::Invalid as i32
    );
    assert_eq!(
        unsafe { crate::sfm_terminal_submit(engine, before.0 + 1, b"FORGED".as_ptr(), 6) },
        crate::Status::Invalid as i32
    );
    assert_eq!(
        std::thread::spawn(move || unsafe {
            crate::sfm_terminal_submit(engine, before.0 + 1, b"FORGED".as_ptr(), 6)
        })
        .join()
        .unwrap(),
        crate::Status::Invalid as i32
    );
    assert_eq!(progress(engine), before);
    let unchanged = crate::engine::with_engine(engine, |owned| {
        owned.terminal.render().map_err(|_| crate::Status::Engine)
    })
    .unwrap();
    assert_eq!(unchanged.cells, before_frame.cells);
    assert_eq!(unchanged.cursor, before_frame.cursor);
    assert_eq!(sfm_manager_v2_terminal_consumed(h.h.handle, engine), 0);
    h.h.dispatch();
    assert!(h.h.mock.requests.is_empty());
    crate::engine::with_engine(engine, crate::engine::pump).unwrap();
    assert_eq!(sfm_manager_v2_terminal_consumed(h.h.handle, engine), 0);
    let ack = h.h.request();
    assert_eq!(ack.body["method"], "acknowledgeTerminal");
    let typed: m::AcknowledgeTerminal =
        serde_json::from_value(ack.body["payload"].clone()).unwrap();
    assert!(matches!(typed.ack, m::Ack::Data { sequence: 1 }));
    h.complete(&ack, h.bound(json!({"acknowledged":true})));
    assert_eq!(h.h.event()["status"], 0);
    let parsed = crate::engine::with_engine(engine, |owned| {
        owned.terminal.render().map_err(|_| crate::Status::Engine)
    })
    .unwrap();
    assert!(parsed
        .cells
        .iter()
        .map(|c| c.text.as_str())
        .collect::<String>()
        .contains("HOST-DATA"));
    assert_eq!(sfm_manager_v2_unbind_terminal(h.h.handle, engine), 0);
    crate::engine::close(engine).unwrap();
}
#[test]
fn closed_manager_engine_rejects_new_and_known_unsent_io_but_allows_detach_cleanup() {
    let mut h = ManagerHarness::new();
    h.pair();
    let value = attachment();
    let m::Attachment::Attached(ref attached) = value else {
        panic!()
    };
    let delivery = attached.delivery.clone();
    let release = attached.input.clone();
    let engine = h.bind_attach_value(value);
    h.ack(engine);
    assert_eq!(
        h.input(engine, json!({"type":"commit","text":"front"})).0,
        0
    );
    let front = h.h.request();
    assert_eq!(
        h.input(engine, json!({"type":"commit","text":"never sent"}))
            .0,
        0
    );
    assert_eq!(crate::sfm_terminal_close(engine), 0);
    let input = h.input(engine, json!({"type":"commit","text":"after close"}));
    assert_ne!(input.0, 0);
    assert_eq!(input.1, 0);
    let mut resize = 999;
    assert_ne!(
        unsafe { sfm_manager_v2_terminal_resize(h.h.handle, engine, 90, 25, &mut resize) },
        0
    );
    assert_eq!(resize, 0);
    let id = front.body["payload"]["inputId"].clone();
    h.complete(
        &front,
        h.bound(json!({"inputId":id,"kind":"key","acceptedBytes":5,"disposition":"accepted"})),
    );
    let mut results = [h.h.event(), h.h.event()];
    results.sort_by_key(|r| r["status"].as_i64().unwrap());
    assert_eq!(results[0]["status"], 0);
    assert_ne!(results[1]["status"], 0);
    h.h.dispatch();
    assert!(h.h.mock.requests.is_empty());
    for command in [
        json!({"method":"attachTerminal","payload":{"tabId":TAB,"resume":null,"control":"ifFree"}}),
        json!({"method":"takeTerminalControl","payload":{"delivery":delivery,"mode":"takeover"}}),
    ] {
        assert_eq!(h.operation(command).0, 0);
        assert_ne!(h.h.event()["status"], 0);
        h.h.dispatch();
        assert!(h.h.mock.requests.is_empty());
    }
    assert_eq!(h.operation(json!({"method":"detachTerminal","payload":{"delivery":delivery,"releaseControl":release}})).0,0);
    let detach = h.h.request();
    assert_eq!(detach.body["method"], "detachTerminal");
    h.complete(&detach, h.bound(json!({"detached":true})));
    assert_eq!(h.h.event()["status"], 0);
    assert_eq!(sfm_manager_v2_unbind_terminal(h.h.handle, engine), 0);
}
