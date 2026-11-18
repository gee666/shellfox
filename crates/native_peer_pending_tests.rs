//! Public SDK/client + actual local VT recovery with wire fixtures. These tests
//! do not claim Runtime-issued grants, peer authentication or Go acceptance.
use super::*;
use shellfox_ghostty_vt::extension::*;
use shellfox_mobile::ManagerRecovery;
use shellfox_protocol::{
    AuthenticatedPeerId, Endpoint, OutboundTransport, RequestLimits, TransportFuture,
    manager_v2 as m,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone)]
struct Fixture {
    codec: m::Native0Codec,
    attachment: m::Attachment,
    writes: Arc<AtomicUsize>,
}
impl OutboundTransport for Fixture {
    fn send<'a>(
        &'a self,
        _: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        _: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let reply = match endpoint {
                Endpoint::DiscoverV2 => {
                    let mut value: serde_json::Value = serde_json::from_str(include_str!(
                        "shellfox-protocol/tests/fixtures/manager_v2/discovery.json"
                    ))
                    .unwrap();
                    value["value"]["recoveryFormats"] = serde_json::json!(["native0-raw-v1"]);
                    value
                }
                Endpoint::PairV2 => {
                    let offer: m::PairOffer = serde_json::from_value(body).unwrap();
                    serde_json::json!({"version":2,"ok":true,"value":m::PairSelection{client_id:offer.client_id,attempt_id:offer.attempt_id,owner_id:offer.owner_id,connection_id:m::Id::new(),profile:m::Profile::Manager,catalog_mode:offer.catalog_mode,delivery_format:offer.delivery_format,recovery_format:offer.recovery_format,input_format:offer.input_format,device_policy:m::DevicePolicy::ControlFullAccount,device_permission:m::DevicePermission::Write,native0_sender:Some(self.codec.clone()),native0_receiver:offer.native0_receiver}})
                }
                Endpoint::RequestV2 => {
                    let value = match body["method"].as_str().unwrap() {
                        "attachTerminal" => serde_json::to_value(&self.attachment).unwrap(),
                        "acknowledgeTerminal" => serde_json::json!({"acknowledged":true}),
                        "writeTerminal" => {
                            self.writes.fetch_add(1, Ordering::SeqCst);
                            let input_id: m::Id =
                                serde_json::from_value(body["payload"]["inputId"].clone()).unwrap();
                            return Ok(serde_json::to_vec(&serde_json::json!({"version":2,"binding":body["binding"],"ok":false,"error":m::Error{code:shellfox_protocol::ErrorCode::NativeUnavailable,message:"Fixture reports an uncertain input outcome".into(),retryable:false,detail:m::ErrorDetail::InputOutcomeUnknown{input_id,retry_input:false}}})).unwrap());
                        }
                        method => panic!("Unexpected fixture method {method}"),
                    };
                    serde_json::json!({"version":2,"binding":body["binding"],"ok":true,"value":value})
                }
                _ => panic!("No networking or fallback in this unit fixture"),
            };
            Ok(serde_json::to_vec(&reply).unwrap())
        })
    }
}
fn fixture() -> Fixture {
    let receiver = VerifiedNativeExtension::verify().unwrap();
    let codec = m::Native0Codec::new(
        m::EngineIdentity::try_from(
            String::from_utf8(receiver.identity().as_bytes().to_vec()).unwrap(),
        )
        .unwrap(),
    );
    let config = RestorationConfig {
        runtime_image_storage_limit: 10_000_000,
        runtime_resize_pull_scrollback: false,
    };
    let mut source = receiver
        .new_native_terminal(20, 5, TerminalProfile::Native, config, 65536)
        .unwrap();
    source.feed(b"actual native owner fixture").unwrap();
    let (snapshot, _) = source
        .encode_native_snapshot(&receiver, SnapshotLimits::default(), config)
        .unwrap();
    let raw: serde_json::Value = serde_json::from_str(include_str!(
        "shellfox-protocol/tests/fixtures/manager_v2/attachment.json"
    ))
    .unwrap();
    let mut attachment: m::Attachment = serde_json::from_value(raw["value"].clone()).unwrap();
    let mut recovery: m::Recovery = serde_json::from_str(include_str!(
        "shellfox-protocol/tests/fixtures/manager_v2/recovery-native0.json"
    ))
    .unwrap();
    let m::Recovery::Native0RawV1 {
        data,
        cols,
        rows,
        metadata,
        ..
    } = &mut recovery
    else {
        panic!("Native0 fixture")
    };
    *data = snapshot
        .as_bytes()
        .chunks(12288)
        .map(|part| shellfox_protocol::encode_chunk(part).unwrap())
        .collect();
    *cols = 20;
    *rows = 5;
    let actual = snapshot.metadata();
    let state = actual.state.unwrap();
    metadata.codec = codec.clone();
    metadata.state = m::Native0State {
        abi_version: state.abi_version,
        terminal_profile: 0,
        preceding_join: state.preceding_join,
    };
    metadata.configuration = m::Native0Configuration {
        runtime_image_storage_limit: actual.configuration.runtime_image_storage_limit,
        runtime_resize_pull_scrollback: actual.configuration.runtime_resize_pull_scrollback,
    };
    let m::Attachment::Attached(attached) = &mut attachment else {
        panic!("Attached fixture")
    };
    attached.recovery = recovery;
    Fixture {
        codec,
        attachment,
        writes: Arc::new(AtomicUsize::new(0)),
    }
}
fn admit(view: &ManagerRecovery<Fixture>) -> PendingManagerInput {
    view.admit_input(
        m::Input::Paste { text: "x".into() },
        m::InputHints {
            user_initiated: true,
            activity_hint: true,
            settled_at: None,
        },
    )
    .unwrap()
}
#[tokio::test(flavor = "current_thread")]
async fn sixteen_slots_front_unknown_cancels_unsent_tail_without_replaying_front() {
    let transport = fixture();
    let writes = transport.writes.clone();
    let client = ManagerRecovery::native_client(
        AuthenticatedPeerId::new("n-unit-fixture").unwrap(),
        m::Id::new(),
        transport.clone(),
    )
    .unwrap();
    client
        .discover(shellfox_client::manager_v2::FallbackPolicy::Never)
        .await
        .unwrap();
    assert!(matches!(
        client.pair(m::Id::new()).await.unwrap(),
        m::Reply::Success(_)
    ));
    let mut view =
        ManagerRecovery::from_shared_native0(client, 20, 5, SnapshotLimits::default()).unwrap();
    let owner = view.owner_view();
    let tab = match &transport.attachment {
        m::Attachment::Attached(value) => value.delivery.tab_id,
        _ => unreachable!(),
    };
    assert!(matches!(
        view.attach(m::AttachTerminal {
            tab_id: tab,
            resume: None,
            control: m::ControlRequest::IfFree
        })
        .await
        .unwrap(),
        m::Reply::Success(_)
    ));
    owner.pump().unwrap();
    assert!(matches!(
        view.acknowledge_consumed().await.unwrap(),
        m::Reply::Success(_)
    ));
    let front = admit(&view);
    let mut tail = PendingInputs::default();
    for _ in 0..15 {
        tail.push(admit(&view));
    }
    assert!(
        view.admit_input(
            m::Input::Paste {
                text: "full".into()
            },
            m::InputHints {
                user_initiated: true,
                activity_hint: true,
                settled_at: None
            }
        )
        .is_err()
    );
    assert!(matches!(
        view.execute_input(front).await.unwrap(),
        m::Reply::Failure(m::Error {
            detail: m::ErrorDetail::InputOutcomeUnknown {
                retry_input: false,
                ..
            },
            ..
        })
    ));
    assert!(tail.cancel_unsent());
    assert_eq!(tail.pending_count(), 0);
    for _ in 0..15 {
        tail.push(admit(&view));
    }
    assert!(
        view.admit_input(
            m::Input::Paste {
                text: "unknown front remains reserved".into()
            },
            m::InputHints {
                user_initiated: true,
                activity_hint: true,
                settled_at: None
            }
        )
        .is_err()
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(tail.cancel_unsent());
    // Exercise early-return/drop cleanup too, while the original SDK is alive.
    for _ in 0..15 {
        tail.push(admit(&view));
    }
    drop(tail);
    let final_input = admit(&view);
    final_input.cancel_before_send().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    owner.close().unwrap();
}
