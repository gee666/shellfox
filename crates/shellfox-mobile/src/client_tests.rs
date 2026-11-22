use super::*;
use serde_json::{json, Value};
use std::{
    ptr,
    sync::{Condvar, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};
#[path = "client_tests_flow.rs"]
mod bounded_flow;
#[path = "client_tests_files.rs"]
mod file_commands;
#[path = "client_tests_input_ex.rs"]
mod input_metadata;
#[path = "client_tests_mutations.rs"]
mod mutation_order;
#[path = "client_tests_terminal.rs"]
mod native_terminal;
#[path = "client_tests_observer.rs"]
mod observer_v2;
#[path = "client_tests_geometry.rs"]
mod recovery_geometry;
#[path = "client_tests_settlement.rs"]
mod settlement_tokens;
#[path = "client_tests_uploads.rs"]
mod upload_fencing;

const PAIR: &[u8] = include_bytes!("../../shellfox-client/tests/fixtures/remote-pair-success.json");
const TAB: &str = "20000000-0000-4000-8000-000000000001";
struct NativeView(u64);
impl NativeView {
    fn attach(h: &mut Harness) -> Self {
        let engine = Self(crate::engine::create(80, 24).unwrap());
        assert_eq!(
            unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), engine.0) },
            0
        );
        assert_eq!(
            h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
                .0,
            0
        );
        let request = h.request();
        let attachment = include_bytes!(
            "../../shellfox-client/tests/fixtures/terminal-attach-owned-success.json"
        );
        assert_eq!(h.complete(&request, attachment), 0);
        let event = h.event();
        assert_eq!(event["status"], 0);
        reset_for_event(h, engine.0, &event);
        crate::engine::with_engine(engine.0, crate::engine::pump).unwrap();
        engine
    }
}
fn reset_for_event(h: &Harness, engine: u64, event: &Value) {
    let key = engine.to_string();
    let grid = &event["resetGeometry"][&key];
    let cols = u16::try_from(grid["cols"].as_u64().unwrap()).unwrap();
    let rows = u16::try_from(grid["rows"].as_u64().unwrap()).unwrap();
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_reset_ticket(h.handle, engine, &mut ticket) },
        0
    );
    crate::engine::reset(engine, cols, rows).unwrap();
    assert_eq!(sfm_client_terminal_reset_done(h.handle, engine, ticket), 0);
}
impl Drop for NativeView {
    fn drop(&mut self) {
        crate::engine::close(self.0).unwrap();
    }
}
#[derive(Debug)]
struct NativeRequest {
    generation: u64,
    id: u64,
    peer: String,
    path: String,
    body: Value,
    limit: usize,
}
#[derive(Default)]
struct MockGo {
    requests: Vec<NativeRequest>,
    cancelled: Vec<(u64, u64)>,
}
unsafe extern "C" fn request(
    context: *mut c_void,
    _: u64,
    generation: u64,
    id: u64,
    peer: *const u8,
    peer_len: usize,
    path: *const u8,
    path_len: usize,
    body: *const u8,
    body_len: usize,
    _: u32,
    limit: usize,
) {
    // Test native-Go seam copies all borrowed bytes before dispatch returns.
    let mock = unsafe { &mut *context.cast::<MockGo>() };
    let copy = |ptr, len| unsafe { slice::from_raw_parts(ptr, len).to_vec() };
    mock.requests.push(NativeRequest {
        generation,
        id,
        peer: String::from_utf8_lossy(&copy(peer, peer_len)).into_owned(),
        path: String::from_utf8_lossy(&copy(path, path_len)).into_owned(),
        body: serde_json::from_slice(&copy(body, body_len)).unwrap_or(Value::Null),
        limit,
    });
}
unsafe extern "C" fn cancel(context: *mut c_void, _: u64, generation: u64, id: u64) {
    unsafe { &mut *context.cast::<MockGo>() }
        .cancelled
        .push((generation, id));
}
struct HarnessPermit;
fn harness_slots() -> &'static (Mutex<usize>, Condvar) {
    static SLOTS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    SLOTS.get_or_init(|| (Mutex::new(0), Condvar::new()))
}
impl HarnessPermit {
    fn acquire() -> Self {
        let (slots, changed) = harness_slots();
        let mut used = slots.lock().unwrap();
        while *used == 4 {
            used = changed.wait(used).unwrap();
        }
        *used += 1;
        Self
    }
}
impl Drop for HarnessPermit {
    fn drop(&mut self) {
        let (slots, changed) = harness_slots();
        *slots.lock().unwrap() -= 1;
        changed.notify_one();
    }
}
struct Harness {
    handle: u64,
    mock: MockGo,
    _permit: HarnessPermit,
}
impl Harness {
    fn new() -> Self {
        let permit = HarnessPermit::acquire();
        let peer = b"n-fake-peer";
        let identity = b"10000000-0000-4000-8000-000000000001";
        let mut handle = 0;
        assert_eq!(
            unsafe {
                sfm_client_create(
                    peer.as_ptr(),
                    peer.len(),
                    identity.as_ptr(),
                    identity.len(),
                    &mut handle,
                )
            },
            0
        );
        Self {
            handle,
            mock: MockGo::default(),
            _permit: permit,
        }
    }
    fn dispatch(&mut self) {
        assert_eq!(
            unsafe {
                sfm_client_dispatch(
                    self.handle,
                    Some(request),
                    Some(cancel),
                    ptr::from_mut(&mut self.mock).cast(),
                )
            },
            0
        );
    }
    fn request(&mut self) -> NativeRequest {
        let start = Instant::now();
        while self.mock.requests.is_empty() {
            self.dispatch();
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "native request timed out"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.mock.requests.remove(0)
    }
    fn complete(&self, request: &NativeRequest, response: &[u8]) -> i32 {
        unsafe {
            sfm_client_complete(
                self.handle,
                request.generation,
                request.id,
                0,
                response.as_ptr(),
                response.len(),
            )
        }
    }
    fn event(&mut self) -> Value {
        let start = Instant::now();
        loop {
            let mut needed = 0;
            let status =
                unsafe { sfm_client_read_event(self.handle, ptr::null_mut(), 0, &mut needed) };
            if status == BUFFER {
                let mut bytes = vec![0; needed];
                assert_eq!(
                    unsafe {
                        sfm_client_read_event(
                            self.handle,
                            bytes.as_mut_ptr(),
                            bytes.len(),
                            &mut needed,
                        )
                    },
                    0
                );
                return serde_json::from_slice(&bytes).unwrap();
            }
            assert_eq!(status, EMPTY);
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "client result timed out"
            );
            self.dispatch();
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn connect(&mut self) {
        let mut operation = 0;
        assert_eq!(
            unsafe { sfm_client_connect(self.handle, &mut operation) },
            0
        );
        let req = self.request();
        assert_eq!(req.path, "/v1/pair");
        assert_eq!(req.peer, "n-fake-peer");
        assert_eq!(req.limit, 8 * 1024 * 1024);
        assert_eq!(req.body["clientId"], "10000000-0000-4000-8000-000000000001");
        assert_eq!(self.complete(&req, PAIR), 0);
        let event = self.event();
        assert_eq!(event["operationId"], operation.to_string());
        assert_eq!(event["status"], 0);
        assert_eq!(event["internal"], false);
        assert_eq!(event["connection"]["status"], "connected");
        assert!(event["connection"]["connectionId"].is_string());
    }
    fn operation(&self, value: Value) -> (i32, u64) {
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut op = 99;
        let status =
            unsafe { sfm_client_operation(self.handle, bytes.as_ptr(), bytes.len(), &mut op) };
        (status, op)
    }
    fn close(&mut self) {
        assert_eq!(
            unsafe {
                sfm_client_close(
                    self.handle,
                    Some(cancel),
                    ptr::from_mut(&mut self.mock).cast(),
                )
            },
            0
        );
        self.handle = 0;
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                sfm_client_close(
                    self.handle,
                    Some(cancel),
                    ptr::from_mut(&mut self.mock).cast(),
                );
            }
        }
    }
}
#[path = "client_tests_transport.rs"]
mod transport_contract;
