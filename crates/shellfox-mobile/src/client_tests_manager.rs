//! Actual C ABI/VT queue with HTTP transport fixtures, not native server grants.
use super::*;
#[path = "client_tests_manager_unicode.rs"]
mod unicode;
#[path = "client_tests_manager_fifo.rs"]
mod fifo;
#[path = "client_tests_manager_role.rs"]
mod role;
use shellfox_ghostty_vt::extension::{
    RestorationConfig, SnapshotLimits, TerminalProfile, VerifiedNativeExtension,
};
use shellfox_protocol::{manager_v2 as m, Validate};
struct ManagerHarness {
    h: Harness,
    binding: Option<m::Binding>,
}
impl ManagerHarness {
    fn new() -> Self {
        let permit = HarnessPermit::acquire();
        let peer = b"n-fake-peer";
        let identity = b"10000000-0000-4000-8000-000000000001";
        let mut handle = 0;
        assert_eq!(
            unsafe {
                sfm_manager_v2_create(
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
            h: Harness {
                handle,
                mock: MockGo::default(),
                _permit: permit,
            },
            binding: None,
        }
    }
    fn complete(&self, request: &NativeRequest, value: Value) {
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            unsafe {
                sfm_manager_v2_complete_http(
                    self.h.handle,
                    request.generation,
                    request.id,
                    0,
                    200,
                    bytes.as_ptr(),
                    bytes.len(),
                )
            },
            0
        );
    }
    fn pair(&mut self) {
        let mut op = 0;
        assert_eq!(
            unsafe { sfm_manager_v2_discover(self.h.handle, &mut op) },
            0
        );
        let request = self.h.request();
        assert_eq!(request.path, "/v2/discover");
        let mut reply: Value = serde_json::from_str(include_str!(
            "../../shellfox-protocol/tests/fixtures/manager_v2/discovery.json"
        ))
        .unwrap();
        reply["value"]["recoveryFormats"] = json!(["native0-raw-v1"]);
        self.complete(&request, reply);
        assert_eq!(self.h.event()["status"], 0);
        let attempt = b"30000000-0000-4000-8000-000000000001";
        assert_eq!(
            unsafe { sfm_manager_v2_pair(self.h.handle, attempt.as_ptr(), attempt.len(), &mut op) },
            0
        );
        let request = self.h.request();
        let offer: m::PairOffer = serde_json::from_value(request.body.clone()).unwrap();
        let selection = m::PairSelection {
            client_id: offer.client_id,
            attempt_id: offer.attempt_id,
            owner_id: offer.owner_id,
            connection_id: "40000000-0000-4000-8000-000000000001".parse().unwrap(),
            profile: m::Profile::Manager,
            catalog_mode: offer.catalog_mode,
            delivery_format: offer.delivery_format,
            recovery_format: offer.recovery_format,
            input_format: offer.input_format,
            device_policy: m::DevicePolicy::ControlFullAccount,
            device_permission: m::DevicePermission::Write,
            native0_sender: offer.native0_receiver.clone(),
            native0_receiver: offer.native0_receiver,
        };
        self.binding = Some(selection.binding());
        self.complete(
            &request,
            serde_json::to_value(m::Reply::Success(selection)).unwrap(),
        );
        assert_eq!(self.h.event()["status"], 0);
    }
    fn operation(&self, value: Value) -> (i32, u64) {
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut op = 0;
        let status = unsafe {
            sfm_manager_v2_operation(self.h.handle, bytes.as_ptr(), bytes.len(), &mut op)
        };
        (status, op)
    }
    fn bound(&self, value: Value) -> Value {
        json!({"version":2,"binding":self.binding,"ok":true,"value":value})
    }
    fn bind_attach(&mut self) -> u64 {
        self.bind_attach_value(attachment())
    }
    fn bind_attach_value(&mut self, value: m::Attachment) -> u64 {
        let engine = crate::engine::create(80, 24).unwrap();
        assert_eq!(
            unsafe { sfm_manager_v2_bind_terminal(self.h.handle, TAB.as_ptr(), TAB.len(), engine) },
            0
        );
        assert_eq!(self.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB,"resume":null,"control":"ifFree"}})).0,0);
        let request = self.h.request();
        self.complete(&request, self.bound(serde_json::to_value(value).unwrap()));
        let event = self.h.event();
        assert_eq!(event["status"], 0);
        assert_eq!(event["profile"], "manager-v2");
        assert!(event["result"]["value"]["recovery"].get("data").is_none());
        engine
    }
    fn ack(&mut self, engine: u64) {
        crate::engine::with_engine(engine, crate::engine::pump).unwrap();
        assert_eq!(sfm_manager_v2_terminal_consumed(self.h.handle, engine), 0);
        let request = self.h.request();
        assert_eq!(request.body["method"], "acknowledgeTerminal");
        self.complete(&request, self.bound(json!({"acknowledged":true})));
        assert_eq!(self.h.event()["status"], 0);
    }
    fn input(&self, engine: u64, value: Value) -> (i32, u64) {
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut operation = 0;
        let status = unsafe {
            sfm_manager_v2_terminal_input(
                self.h.handle,
                engine,
                bytes.as_ptr(),
                bytes.len(),
                1,
                &mut operation,
            )
        };
        (status, operation)
    }
}
fn attachment() -> m::Attachment {
    let verified = VerifiedNativeExtension::verify().unwrap();
    let config = RestorationConfig {
        runtime_image_storage_limit: 10_000_000,
        runtime_resize_pull_scrollback: false,
    };
    let mut source = verified
        .new_native_terminal(80, 24, TerminalProfile::Native, config, 65536)
        .unwrap();
    source
        .feed(b"CABI native source\x1b[?1049halt\x1b[38;2;")
        .unwrap();
    attachment_source(&mut source, config)
}
fn attachment_source(
    source: &mut shellfox_ghostty_vt::Terminal,
    config: RestorationConfig,
) -> m::Attachment {
    let verified = VerifiedNativeExtension::verify().unwrap();
    let (snapshot, _) = source
        .encode_native_snapshot(&verified, SnapshotLimits::default(), config)
        .unwrap();
    let mut data = String::new();
    for part in snapshot.as_bytes().chunks(12288) {
        data.push_str(&shellfox_protocol::encode_chunk(part).unwrap());
    }
    let bound: m::BoundReply<m::Attachment> = shellfox_protocol::decode(include_bytes!(
        "../../shellfox-protocol/tests/fixtures/manager_v2/attachment.json"
    ))
    .unwrap();
    let m::Reply::Success(mut value) = bound.outcome else {
        panic!()
    };
    let mut recovery: m::Recovery = serde_json::from_str(include_str!(
        "../../shellfox-protocol/tests/fixtures/manager_v2/recovery-native0.json"
    ))
    .unwrap();
    let m::Recovery::Native0RawV1 {
        data: body,
        metadata,
        ..
    } = &mut recovery
    else {
        panic!()
    };
    *body = data;
    metadata.codec = m::Native0Codec::new(
        m::EngineIdentity::try_from(
            String::from_utf8(verified.identity().as_bytes().to_vec()).unwrap(),
        )
        .unwrap(),
    );
    let original = snapshot.metadata();
    let state = original.state.unwrap();
    metadata.state = m::Native0State {
        abi_version: state.abi_version,
        terminal_profile: 0,
        preceding_join: state.preceding_join,
    };
    metadata.configuration = m::Native0Configuration {
        runtime_image_storage_limit: original.configuration.runtime_image_storage_limit,
        runtime_resize_pull_scrollback: original.configuration.runtime_resize_pull_scrollback,
    };
    let m::Attachment::Attached(attached) = &mut value else {
        panic!()
    };
    // Keep the response tied to the exact tab admitted by bind/attach.
    let tab = TAB.parse().unwrap();
    attached.delivery.tab_id = tab;
    if let Some(input) = &mut attached.input {
        input.tab_id = tab;
    }
    if let Some(layout) = &mut attached.layout {
        layout.tab_id = tab;
    }
    attached.recovery = recovery;
    value.validate().unwrap();
    value
}
#[test]
fn manager_cabi_installs_before_ack_and_host_encodes_committed_input() {
    let mut h = ManagerHarness::new();
    h.pair();
    let engine = h.bind_attach();
    let before = h.input(engine, json!({"type":"commit","text":"a"}));
    assert_ne!(before.0, 0);
    assert_eq!(before.1, 0);
    h.ack(engine);
    let (status, operation) = h.input(engine, json!({"type":"commit","text":"é"}));
    assert_eq!(status, 0);
    assert_ne!(operation, 0);
    let request = h.h.request();
    assert_eq!(request.body["payload"]["input"]["kind"], "key");
    assert_eq!(request.body["payload"]["input"]["event"]["text"], "é");
    assert_eq!(request.body["payload"]["hints"]["userInitiated"], true);
    assert!(request.body["payload"]["hints"].get("settledAt").is_none());
    let id = request.body["payload"]["inputId"].clone();
    h.complete(
        &request,
        h.bound(json!({"inputId":id,"kind":"key","acceptedBytes":2,"disposition":"accepted"})),
    );
    assert_eq!(h.h.event()["status"], 0);
    assert_eq!(sfm_manager_v2_unbind_terminal(h.h.handle, engine), 0);
    crate::engine::close(engine).unwrap();
}
#[test]
fn manager_cabi_rejects_js_ack_and_one_view_unbind_keeps_global_pair() {
    let mut h = ManagerHarness::new();
    h.pair();
    let engine = h.bind_attach();
    h.ack(engine);
    let op=h.operation(json!({"method":"acknowledgeTerminal","payload":{"delivery":{"tabId":TAB,"generation":"30000000-0000-4000-8000-000000000001","deliveryId":"70000000-0000-4000-8000-000000000001","deliveryEpoch":1},"ack":{"kind":"data","sequence":1}}}));
    assert_ne!(op.0, 0);
    assert_eq!(op.1, 0);
    let second = crate::engine::create(80, 24).unwrap();
    let tab = b"90000000-0000-4000-8000-000000000001";
    assert_eq!(
        unsafe { sfm_manager_v2_bind_terminal(h.h.handle, tab.as_ptr(), tab.len(), second) },
        0
    );
    assert_eq!(sfm_manager_v2_unbind_terminal(h.h.handle, second), 0);
    crate::engine::close(second).unwrap();
    assert_eq!(
        registry_lookup(h.h.handle)
            .unwrap()
            .manager()
            .unwrap()
            .binding(),
        h.binding
    );
    assert_eq!(sfm_manager_v2_unbind_terminal(h.h.handle, engine), 0);
    crate::engine::close(engine).unwrap();
}
