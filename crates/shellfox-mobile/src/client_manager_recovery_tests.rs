//! Local VT/SDK queue plus transport fixtures, not Runtime grants or native auth proof.
use super::*;
use transport_fixture::*;
mod transport_fixture {
    use super::*;
    use shellfox_protocol::*;
    pub enum AttachmentReply {
        Ready(m::Attachment),
        Delayed(tokio::sync::oneshot::Receiver<m::Attachment>),
    }
    #[derive(Default)]
    pub struct Replies {
        pub calls: Vec<serde_json::Value>,
        pub attachments: VecDeque<AttachmentReply>,
        pub polls: VecDeque<m::PollResponse>,
    }
    #[derive(Clone)]
    pub struct Transport(pub Arc<Mutex<Replies>>, pub m::Native0Codec);
    impl Transport {
        pub fn ready(&self, value: m::Attachment) {
            self.0
                .lock()
                .unwrap()
                .attachments
                .push_back(AttachmentReply::Ready(value));
        }
        pub fn calls(&self) -> usize {
            self.0.lock().unwrap().calls.len()
        }
        pub fn event(&self, event: m::Event) {
            self.0.lock().unwrap().polls.push_back(m::PollResponse {
                events: vec![m::HostEvent::Terminal { event }],
            });
        }
    }
    impl OutboundTransport for Transport {
        fn send<'a>(
            &'a self,
            _peer: &'a AuthenticatedPeerId,
            endpoint: Endpoint,
            body: Vec<u8>,
            _limits: RequestLimits,
        ) -> TransportFuture<'a> {
            Box::pin(async move {
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                self.0.lock().unwrap().calls.push(body.clone());
                let value = match endpoint {
                    Endpoint::DiscoverV2 => {
                        serde_json::json!({"version":2,"ok":true,"value":{"service":"shellfox","ownerId":id(1),"profiles":["manager-v2"],"catalogModes":["complete"],"deliveryFormats":["vt-bytes-base64-v1"],"recoveryFormats":["native0-raw-v1"]}})
                    }
                    Endpoint::PairV2 => {
                        let offer: m::PairOffer = serde_json::from_value(body).unwrap();
                        serde_json::json!({"version":2,"ok":true,"value":m::PairSelection{client_id:offer.client_id,attempt_id:offer.attempt_id,owner_id:offer.owner_id,connection_id:id(4),profile:m::Profile::Manager,catalog_mode:offer.catalog_mode,delivery_format:offer.delivery_format,recovery_format:offer.recovery_format,input_format:offer.input_format,device_policy:m::DevicePolicy::ControlFullAccount,device_permission:m::DevicePermission::Write,native0_sender:Some(self.1.clone()),native0_receiver:offer.native0_receiver}})
                    }
                    Endpoint::RequestV2 => {
                        if body["method"]=="writeTerminal" {
                            return Ok(serde_json::to_vec(&serde_json::json!({"version":2,"binding":body["binding"],"ok":false,"error":m::Error{code:ErrorCode::Validation,message:"Fixture rejects input".to_owned(),retryable:false,detail:m::ErrorDetail::Application}})).unwrap());
                        }
                        let value = if body["method"] == "attachTerminal" {
                            let next = self
                                .0
                                .lock()
                                .unwrap()
                                .attachments
                                .pop_front()
                                .expect("explicit attachment response fixture");
                            let value = match next {
                                AttachmentReply::Ready(value) => value,
                                AttachmentReply::Delayed(receiver) => receiver.await.unwrap(),
                            };
                            serde_json::to_value(value).unwrap()
                        } else {
                            assert_eq!(body["method"], "acknowledgeTerminal");
                            serde_json::json!({"acknowledged":true})
                        };
                        serde_json::json!({"version":2,"binding":body["binding"],"ok":true,"value":value})
                    }
                    Endpoint::EventsV2 => {
                        let events = self
                            .0
                            .lock()
                            .unwrap()
                            .polls
                            .pop_front()
                            .unwrap_or(m::PollResponse { events: vec![] });
                        serde_json::json!({"version":2,"binding":body["binding"],"ok":true,"value":events})
                    }
                    _ => panic!("no automatic fallback/input/network"),
                };
                Ok(serde_json::to_vec(&value).unwrap())
            })
        }
    }
    pub fn id(n: u8) -> m::Id {
        format!("{n}0000000-0000-4000-8000-000000000001")
            .parse()
            .unwrap()
    }
    pub fn request() -> m::AttachTerminal {
        m::AttachTerminal {
            tab_id: id(5),
            resume: None,
            control: m::ControlRequest::None,
        }
    }
    pub async fn pair(manager: &ManagerRecovery<Transport>, attempt: m::Id) {
        manager
            .discover(shellfox_client::manager_v2::FallbackPolicy::Never)
            .await
            .unwrap();
        manager.pair(attempt).await.unwrap();
    }
    pub async fn manager() -> (ManagerRecovery<Transport>, Transport) {
        let verified = VerifiedNativeExtension::verify().unwrap();
        let identity = String::from_utf8(verified.identity().as_bytes().to_vec()).unwrap();
        let transport = Transport(
            Arc::new(Mutex::new(Replies::default())),
            m::Native0Codec::new(m::EngineIdentity::try_from(identity).unwrap()),
        );
        let manager = ManagerRecovery::native0(
            AuthenticatedPeerId::new("fixture-peer").unwrap(),
            id(2),
            transport.clone(),
            20,
            5,
            SnapshotLimits::default(),
        )
        .unwrap();
        pair(&manager, id(3)).await;
        (manager, transport)
    }
    pub fn source() -> shellfox_ghostty_vt::Terminal {
        use shellfox_ghostty_vt::extension::*;
        let verified = VerifiedNativeExtension::verify().unwrap();
        let config = RestorationConfig {
            runtime_image_storage_limit: 10_000_000,
            runtime_resize_pull_scrollback: false,
        };
        let mut terminal = verified
            .new_native_terminal(20, 5, TerminalProfile::Native, config, 65536)
            .unwrap();
        terminal
            .feed(b"primary\x1b[2;2H\x1b7\x1b[?1049halt\x1b[38;2;")
            .unwrap();
        terminal
    }
    pub fn attachment(transport: &Transport) -> m::Attachment {
        use shellfox_ghostty_vt::extension::*;
        let verified = VerifiedNativeExtension::verify().unwrap();
        let config = RestorationConfig {
            runtime_image_storage_limit: 10_000_000,
            runtime_resize_pull_scrollback: false,
        };
        let (snapshot, _) = source()
            .encode_native_snapshot(&verified, SnapshotLimits::default(), config)
            .unwrap();
        attachment_snapshot(transport, &snapshot)
    }
    pub fn attachment_snapshot(
        transport: &Transport,
        snapshot: &shellfox_ghostty_vt::extension::NativeSnapshot,
    ) -> m::Attachment {
        let mut data = String::new();
        for part in snapshot.as_bytes().chunks(12288) {
            data.push_str(&shellfox_protocol::encode_chunk(part).unwrap());
        }
        // Plain wire fixture, not a replacement for the production Service mapper.
        let mut recovery: m::Recovery = serde_json::from_str(include_str!(
            "../../shellfox-protocol/tests/fixtures/manager_v2/recovery-native0.json"
        ))
        .unwrap();
        let m::Recovery::Native0RawV1 {
            data: body,
            cols,
            rows,
            metadata,
            ..
        } = &mut recovery
        else {
            panic!()
        };
        *body = data;
        *cols = 20;
        *rows = 5;
        metadata.codec = transport.1.clone();
        let original = snapshot.metadata();
        let state = original.state.expect("actual Native0 State");
        metadata.state = m::Native0State {
            abi_version: state.abi_version,
            terminal_profile: 0,
            preceding_join: state.preceding_join,
        };
        metadata.configuration = m::Native0Configuration {
            runtime_image_storage_limit: original.configuration.runtime_image_storage_limit,
            runtime_resize_pull_scrollback: original.configuration.runtime_resize_pull_scrollback,
        };
        m::Attachment::Attached(Box::new(m::Attached {
            delivery: m::DeliveryRef {
                tab_id: id(5),
                generation: id(6),
                delivery_id: id(7),
                delivery_epoch: 1,
            },
            input: None,
            input_state: m::InputState::NotReady,
            layout: None,
            recovery,
        }))
    }
    pub fn old(manager: &ManagerRecovery<Transport>) -> shellfox_ghostty_vt::Screen {
        crate::engine::with_engine(manager.terminal, |owned| {
            owned.terminal.feed(b"OLD VIEW").unwrap();
            owned.terminal.render().map_err(|_| crate::Status::Engine)
        })
        .unwrap()
    }
    pub fn progress(manager: &ManagerRecovery<Transport>) -> (u64, u64) {
        let shared = crate::engine::lookup(manager.terminal).unwrap();
        let queue = shared.queue.lock().unwrap();
        (queue.accepted, queue.consumed)
    }
    pub fn unchanged(manager: &ManagerRecovery<Transport>, before: &shellfox_ghostty_vt::Screen) {
        let now = crate::engine::with_engine(manager.terminal, |owned| {
            owned.terminal.render().map_err(|_| crate::Status::Engine)
        })
        .unwrap();
        assert_eq!(now.cells, before.cells);
        assert_eq!(now.cursor, before.cursor);
        assert_eq!(progress(manager).1, 0);
    }
}
#[test]
fn actual_sdk_install_precedes_genuine_consumption_ack() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mut manager, transport) = manager().await;
        let value = attachment(&transport);
        transport.ready(value.clone());
        manager.attach(request()).await.unwrap();
        let before = transport.calls();
        assert!(manager.acknowledge_consumed().await.is_err());
        let prepared = manager.prepare_pending().unwrap();
        assert!(manager.acknowledge_consumed().await.is_err());
        assert_eq!(transport.calls(), before);
        manager.commit_prepared(prepared).unwrap();
        manager.acknowledge_consumed().await.unwrap();
        let suffix = b"2;3mZ\x1b[?1049l\x1b8X";
        let mut original = source();
        original.feed(suffix).unwrap();
        let expected = original.render().unwrap();
        let m::Attachment::Attached(attached) = value else {
            panic!()
        };
        transport.event(m::Event::Data {
            delivery: attached.delivery,
            sequence: 1,
            encoding: m::Encoding::Base64,
            data: shellfox_protocol::encode_chunk(suffix).unwrap(),
        });
        manager.poll().await.unwrap();
        assert!(manager.acknowledge_consumed().await.is_err());
        manager.pump().unwrap();
        let actual = crate::engine::with_engine(manager.terminal, |owned| {
            owned.terminal.render().map_err(|_| crate::Status::Engine)
        })
        .unwrap();
        assert_eq!(actual.cells, expected.cells);
        assert_eq!(actual.cursor, expected.cursor);
        manager.acknowledge_consumed().await.unwrap();
        manager.close().unwrap();
    });
}
#[test]
fn actual_binding_invalidation_and_cancel_during_staged_decode_keep_old_target() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for rebind in [false, true] {
            let (mut manager, transport) = manager().await;
            let before = old(&manager);
            transport.ready(attachment(&transport));
            manager.attach(request()).await.unwrap();
            let prepared = manager.prepare_pending().unwrap();
            let calls = transport.calls();
            if rebind {
                manager.client().reconnect(id(9));
            } else {
                manager.cancel_pending();
            }
            assert!(manager.commit_prepared(prepared).is_err());
            unchanged(&manager, &before);
            assert!(manager.acknowledge_consumed().await.is_err());
            assert_eq!(transport.calls(), calls);
            manager.close().unwrap();
        }
    });
}
#[test]
fn malformed_profile_budget_and_wrong_negotiated_sender_never_ack_or_replace() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for mutation in 0..4 {
            let (mut manager, transport) = manager().await;
            let before = old(&manager);
            let mut value = attachment(&transport);
            let m::Attachment::Attached(a) = &mut value else {
                panic!()
            };
            let m::Recovery::Native0RawV1 { data, metadata, .. } = &mut a.recovery else {
                panic!()
            };
            match mutation {
                0 => *data = "AA==".into(),
                1 => metadata.state.terminal_profile = 1,
                2 => metadata.limits.max_allocation_bytes = 0,
                _ => {
                    metadata.codec.engine_identity =
                        m::EngineIdentity::try_from("f".repeat(64)).unwrap()
                }
            }
            transport.ready(value);
            if manager.attach(request()).await.is_ok() {
                assert!(manager.prepare_pending().is_err());
            }
            unchanged(&manager, &before);
            let calls = transport.calls();
            assert!(manager.acknowledge_consumed().await.is_err());
            assert_eq!(transport.calls(), calls);
            manager.close().unwrap();
        }
    });
}
#[path = "client_manager_origin_tests.rs"]
mod origins;
#[path = "client_manager_unicode_tests.rs"]
mod unicode;
#[path = "client_manager_mouse_tests.rs"]
mod mouse;
#[path = "client_manager_target_tests.rs"]
mod target;
#[path = "client_manager_association_tests.rs"]
mod association;
