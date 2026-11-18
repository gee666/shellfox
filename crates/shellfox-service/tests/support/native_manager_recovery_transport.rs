//! Only the ignored boundary's actual Runtime/production-mapper transport fixture.
use super::*;
pub(super) struct Server {
    pub(super) runtime: Runtime,
    pub(super) book: Option<ManagerDelivery>,
    pub(super) lease: Option<Lease>,
    pub(super) selection: Option<m::PairSelection>,
    pub(super) sender: m::Native0Codec,
    pub(super) next: u64,
    pub(super) acks: usize,
}
impl Server {
    fn record(&mut self, event: Event) {
        if let Some(book) = &mut self.book {
            book.record(self.runtime.identity(), event);
        }
    }
    pub(super) fn close_owned(&mut self) {
        if self.runtime.phase() != Phase::Closed {
            let mut request = Request {
                id: RequestId(self.next),
                command: Command::RetryClose,
            };
            self.next += 1;
            loop {
                match self.runtime.try_send(request) {
                    Ok(()) => break,
                    Err(shellfox_runtime::SubmitError::Full(retained)) => request = retained,
                    Err(shellfox_runtime::SubmitError::Disconnected(_)) => break,
                    Err(shellfox_runtime::SubmitError::Invalid(error)) => {
                        panic!("invalid close: {error}")
                    }
                }
                while let Ok(event) = self.runtime.try_recv() {
                    self.record(event);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        let mut warning = Instant::now() + Duration::from_secs(15);
        while self.runtime.phase() != Phase::Closed {
            while let Ok(event) = self.runtime.try_recv() {
                self.record(event);
            }
            if Instant::now() >= warning {
                eprintln!("BOUNDARY_CLEANUP_UNCONFIRMED retaining original Runtime/PTY ownership, phase {:?}",self.runtime.phase());
                warning = Instant::now() + Duration::from_secs(15);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        eprintln!(
            "BOUNDARY_ORIGINAL_RUNTIME_CLOSED: owned child/job/PTY shutdown confirmed by Runtime"
        );
    }
    pub(super) fn request(
        &mut self,
        command: Command,
    ) -> Result<Response, shellfox_runtime::RuntimeError> {
        let id = RequestId(self.next);
        self.next += 1;
        self.runtime.try_send(Request { id, command }).unwrap();
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                Instant::now() < end,
                "original Runtime outcome remains uncertain; retain fixture"
            );
            match self.runtime.try_recv() {
                Ok(Event::Reply { id: actual, result }) if actual == id => {
                    match &result {
                        Ok(Response::Claimed(lease)) => {
                            eprintln!("BOUNDARY_ACTUAL_CLAIM {lease:?}")
                        }
                        Ok(Response::Native0Recovery(native)) => {
                            eprintln!("BOUNDARY_ACTUAL_NATIVE_CUT {:?}", native.cut)
                        }
                        _ => {}
                    }
                    return result;
                }
                Ok(Event::Started(Err(error))) => panic!("original owned startup failed: {error}"),
                Ok(event) => self.record(event),
                Err(_) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }
    pub(super) fn events(&mut self) -> Vec<m::Event> {
        while let Ok(event) = self.runtime.try_recv() {
            self.record(event);
        }
        self.book
            .as_mut()
            .unwrap()
            .poll(m::MAX_POLL_DATA_BYTES, 256)
            .unwrap()
    }
}
#[derive(Clone)]
pub(super) struct Transport(pub(super) Arc<Mutex<Server>>);
impl OutboundTransport for Transport {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        _limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            assert_eq!(peer.as_str(), "owned-fixture-peer");
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let mut server = self.0.lock().unwrap();
            let response = match endpoint {
                Endpoint::DiscoverV2 => {
                    serde_json::json!({"version":2,"ok":true,"value":{"service":"shellfox","ownerId":id(1),"profiles":["manager-v2"],"catalogModes":["complete"],"deliveryFormats":["vt-bytes-base64-v1"],"recoveryFormats":["native0-raw-v1"]}})
                }
                Endpoint::PairV2 => {
                    let offer: m::PairOffer = serde_json::from_value(value).unwrap();
                    let selected = m::PairSelection {
                        client_id: offer.client_id,
                        attempt_id: offer.attempt_id,
                        owner_id: offer.owner_id,
                        connection_id: id(4),
                        profile: m::Profile::Manager,
                        catalog_mode: offer.catalog_mode,
                        delivery_format: offer.delivery_format,
                        recovery_format: offer.recovery_format,
                        input_format: offer.input_format,
                        device_policy: m::DevicePolicy::ControlFullAccount,
                        device_permission: m::DevicePermission::Write,
                        native0_sender: Some(server.sender.clone()),
                        native0_receiver: offer.native0_receiver,
                    };
                    server.selection = Some(selected.clone());
                    serde_json::json!({"version":2,"ok":true,"value":selected})
                }
                Endpoint::RequestV2 => {
                    let request = m::decode_request(&body).unwrap();
                    assert_eq!(
                        Some(&request.binding),
                        server.selection.as_ref().map(|s| s.binding()).as_ref()
                    );
                    let result = match request.command {
                        m::Command::AttachTerminal(input) => {
                            assert_eq!(
                                input.tab_id.to_string(),
                                server.runtime.identity().tab_id.to_string()
                            );
                            let lease = if let Some(lease) = server.lease {
                                lease
                            } else {
                                let Response::Claimed(lease) = server
                                    .request(Command::Claim {
                                        controller: 77,
                                        takeover: false,
                                    })
                                    .unwrap()
                                else {
                                    panic!("real claim")
                                };
                                server.lease = Some(lease);
                                lease
                            };
                            server.book = Some(
                                ManagerDelivery::new(
                                    server.runtime.identity(),
                                    Role::Controller(lease),
                                )
                                .unwrap(),
                            );
                            eprintln!(
                                "BOUNDARY_ACTUAL_SOURCE phase {:?}, launch {:?}",
                                server.runtime.phase(),
                                server.runtime.launch_privilege_facts()
                            );
                            let native = server
                                .request(Command::RecoverNative0Stamped { lease })
                                .unwrap();
                            let selected = server.selection.clone().unwrap();
                            let book = server.book.as_mut().unwrap();
                            let recovery = book.recover_native0(native, &selected).unwrap();
                            serde_json::to_value(m::Attachment::Attached(Box::new(m::Attached {
                                delivery: book.reference().clone(),
                                input: book.control().cloned(),
                                input_state: m::InputState::Owned,
                                layout: book.layout().cloned(),
                                recovery,
                            })))
                            .unwrap()
                        }
                        m::Command::AcknowledgeTerminal(ack) => {
                            let command = server
                                .book
                                .as_ref()
                                .unwrap()
                                .ack_command(&ack.delivery, &ack.ack)
                                .unwrap();
                            eprintln!("BOUNDARY_ORIGINAL_ACK_COMMAND {command:?}");
                            let actual = server.request(command).unwrap();
                            server
                                .book
                                .as_mut()
                                .unwrap()
                                .commit_ack(&ack.delivery, &ack.ack, &actual)
                                .unwrap();
                            assert!(matches!(actual, Response::Applied));
                            server.acks += 1;
                            serde_json::json!({"acknowledged":true})
                        }
                        _ => panic!("no automatic retry/input/grant fallback"),
                    };
                    serde_json::json!({"version":2,"binding":request.binding,"ok":true,"value":result})
                }
                Endpoint::EventsV2 => {
                    assert_eq!(
                        value["binding"],
                        serde_json::to_value(server.selection.as_ref().unwrap().binding()).unwrap()
                    );
                    let events = server
                        .events()
                        .into_iter()
                        .map(|event| m::HostEvent::Terminal { event })
                        .collect::<Vec<_>>();
                    serde_json::json!({"version":2,"binding":value["binding"],"ok":true,"value":m::PollResponse{events}})
                }
                _ => panic!("not a real network or unrestricted transport"),
            };
            Ok(serde_json::to_vec(&response).unwrap())
        })
    }
}
