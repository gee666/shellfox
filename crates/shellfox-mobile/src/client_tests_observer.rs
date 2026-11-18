use super::*;
#[path = "client_tests_observer_pressure.rs"]
mod bounded_credit;
#[path = "client_tests_observer_models.rs"]
mod canonical_models;
#[path = "client_tests_observer_delivery.rs"]
mod delivery_contract;
#[path = "client_tests_observer_transport.rs"]
mod status_transport;

fn same_json(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && b.iter()
                    .all(|(key, value)| a.get(key).is_some_and(|a| same_json(a, value)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_json(a, b))
        }
        _ => actual == expected,
    }
}
fn capture_for_ts(vectors: &[Value], handshake: &[Value], native_acks: &[Value]) {
    if std::env::var("SHELLFOX_CAPTURE_OBSERVER_CABI").as_deref() != Ok("1") {
        return;
    }
    // Explicit fixture-only handoff. No production file access or runtime log.
    assert_eq!(vectors.len(), 43);
    let bytes = serde_json::to_vec(&json!({"captureVersion":1,"wireVersion":2,
        "observerAbiVersion":sfm_observer_v2_abi_version(),"legacyAbiVersion":sfm_client_abi_version(),
        "profile":"observer-v2","handshake":handshake,"nativeAcks":native_acks,"vectors":vectors})).unwrap();
    assert!(
        bytes.len() <= 16 * 1024 * 1024,
        "bounded fixture-only capture"
    );
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/observer-v2-crosslang");
    std::fs::create_dir_all(&directory).unwrap();
    let id = ClientId::new_v4();
    let path = directory.join(format!("capture-{id}.json"));
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let scratch = Scratch(directory.join(format!("capture-{id}.pending")));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&scratch.0)
        .unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    // Publish a complete file without ever replacing another capture.
    std::fs::hard_link(&scratch.0, &path).unwrap();
    eprintln!("SFM_OBSERVER_CABI_CAPTURE={}", path.display());
}
const CLIENT: &str = "20000000-0000-4000-8000-000000000001";
const ATTEMPT: &str = "30000000-0000-4000-8000-000000000001";
const OBS_TAB: &str = "50000000-0000-4000-8000-000000000001";
macro_rules! golden {
    ($name:literal) => {
        include_bytes!(concat!(
            "../../shellfox-protocol/tests/fixtures/observer_v2/",
            $name
        ))
    };
}
fn golden_json(name: &str) -> Value {
    let bytes: &[u8] = match name {
        "binding" => golden!("binding.json"),
        "offer" => golden!("pair-offer.json"),
        "selection" => golden!("pair-selection.json"),
        "attach" => golden!("attachment.json"),
        "events" => golden!("events.json"),
        "ack" => golden!("acknowledged.json"),
        "snapshot" => golden!("snapshot.json"),
        "methods" => golden!("methods.json"),
        "layout" => golden!("layout.json"),
        "exited" => golden!("process-exited.json"),
        _ => panic!("unknown test fixture"),
    };
    serde_json::from_slice(bytes).unwrap()
}
fn new_observer() -> Harness {
    let permit = HarnessPermit::acquire();
    let peer = b"n-fake-peer";
    let mut handle = 0;
    assert_eq!(
        unsafe {
            sfm_observer_v2_create(
                peer.as_ptr(),
                peer.len(),
                CLIENT.as_ptr(),
                CLIENT.len(),
                &mut handle,
            )
        },
        0
    );
    Harness {
        handle,
        mock: MockGo::default(),
        _permit: permit,
    }
}
fn http(h: &Harness, request: &NativeRequest, status: u16, bytes: &[u8]) -> i32 {
    unsafe {
        sfm_observer_v2_complete_http(
            h.handle,
            request.generation,
            request.id,
            0,
            status,
            bytes.as_ptr(),
            bytes.len(),
        )
    }
}
fn json_reply(h: &Harness, request: &NativeRequest, value: &Value) -> i32 {
    http(h, request, 200, &serde_json::to_vec(value).unwrap())
}
fn discover(h: &Harness, policy: u8) -> (i32, u64) {
    let mut operation = 99;
    let result = unsafe { sfm_observer_v2_discover(h.handle, policy, &mut operation) };
    (result, operation)
}
fn connect(h: &mut Harness) {
    let _ = connect_trace(h);
}
fn connect_trace(h: &mut Harness) -> Vec<Value> {
    let mut trace = Vec::new();
    assert_eq!(discover(h, 0).0, 0);
    let request = h.request();
    assert_eq!(request.path, "/v2/discover");
    assert_eq!(request.body, json!({}));
    assert_eq!(http(h, &request, 200, golden!("discovery.json")), 0);
    let event = h.event();
    assert_eq!(event["connection"]["status"], "discovered");
    assert_eq!(event["profile"], "observer-v2");
    assert!(event["connection"]["binding"].is_null());
    trace.push(json!({"kind":"discover","request":request.body,"event":event}));
    let mut operation = 0;
    assert_eq!(
        unsafe { sfm_observer_v2_pair(h.handle, ATTEMPT.as_ptr(), ATTEMPT.len(), &mut operation) },
        0
    );
    let pair = h.request();
    assert_eq!(pair.path, "/v2/pair");
    assert_eq!(pair.body, golden_json("offer"));
    assert_eq!(http(h, &pair, 200, golden!("pair-selection.json")), 0);
    let event = h.event();
    assert_eq!(event["status"], 0);
    assert_eq!(event["connection"]["status"], "connected");
    assert_eq!(event["connection"]["binding"], golden_json("binding"));
    assert!(event["connection"].get("permission").is_none());
    trace.push(json!({"kind":"pair","request":pair.body,"event":event}));
    trace
}
fn operation(h: &Harness, value: Value) -> (i32, u64) {
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut operation = 99;
    let result =
        unsafe { sfm_observer_v2_operation(h.handle, bytes.as_ptr(), bytes.len(), &mut operation) };
    (result, operation)
}
fn view(h: &Harness) -> NativeView {
    let view = NativeView(crate::engine::create(20, 5).unwrap());
    assert_eq!(
        unsafe { sfm_observer_v2_bind_terminal(h.handle, OBS_TAB.as_ptr(), OBS_TAB.len(), view.0) },
        0
    );
    view
}
fn attach(h: &mut Harness, value: &Value) -> Value {
    assert_eq!(
        operation(
            h,
            json!({"method":"attachTerminal","payload":{"tabId":OBS_TAB,"resume":null}})
        )
        .0,
        0
    );
    let request = h.request();
    assert_eq!(request.path, "/v2/request");
    assert_eq!(request.body["binding"], golden_json("binding"));
    assert_eq!(json_reply(h, &request, value), 0);
    let event = h.event();
    assert_eq!(event["status"], 0);
    event
}
fn reset(h: &Harness, view: &NativeView) {
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_observer_v2_terminal_reset_ticket(h.handle, view.0, &mut ticket) },
        0
    );
    let (mut cols, mut rows) = (0, 0);
    let status = unsafe {
        sfm_observer_v2_terminal_reset_grid(h.handle, view.0, ticket, &mut cols, &mut rows)
    };
    if status == EMPTY {
        let grid = super::super::observer_owner::grid(view.0).unwrap();
        cols = grid.0;
        rows = grid.1;
    } else {
        assert_eq!(status, 0);
    }
    crate::engine::reset(view.0, cols, rows).unwrap();
    assert_eq!(
        sfm_observer_v2_terminal_reset_done(h.handle, view.0, ticket),
        0
    );
}
fn pump(view: &NativeView) {
    crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
}
fn ack_recovery(h: &mut Harness, view: &NativeView) {
    let _ = ack_recovery_trace(h, view);
}
fn ack_recovery_trace(h: &mut Harness, view: &NativeView) -> Value {
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), 0);
    let request = h.request();
    assert_eq!(request.body["method"], "acknowledgeTerminal");
    assert_eq!(request.body["payload"]["ack"]["kind"], "recovery");
    assert_eq!(http(h, &request, 200, golden!("acknowledged.json")), 0);
    let event = h.event();
    assert_eq!(event["status"], 0);
    assert_eq!(event["internal"], true);
    json!({"request":request.body,"event":event})
}
fn poll(h: &Harness) -> (i32, u64) {
    let mut operation = 99;
    let status = unsafe { sfm_observer_v2_poll(h.handle, &mut operation) };
    (status, operation)
}
