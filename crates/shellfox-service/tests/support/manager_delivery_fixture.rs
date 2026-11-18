use super::*;
/// One actual in-memory ExternalBytes Runtime, one source, one receiver.
/// No PTY, native launch policy, controller grant or input-capable source.
pub(super) struct Fixture {
    pub runtime: Runtime,
    pub source: ExternalSource,
    pub mailboxes: ManagerMailboxes,
    pub observer: ObserverLease,
    pub reference: wire::DeliveryRef,
    next: u64,
    source_sequence: u64,
}
impl Fixture {
    pub fn new() -> Self {
        Self::with_provisional(false)
    }
    pub fn new_provisional() -> Self {
        Self::with_provisional(true)
    }
    fn with_provisional(provisional: bool) -> Self {
        let mut runtime = Runtime::start_external(identity(), ExternalConfig::default()).unwrap();
        let source = runtime.take_source().unwrap();
        let mut mailboxes = ManagerMailboxes::default();
        let Response::Observed(observer) = request(
            &runtime,
            &mut mailboxes,
            1,
            Command::Observe {
                viewer: 41,
                after: None,
            },
        )
        .unwrap() else {
            panic!("actual observer")
        };
        let entry = ManagerDelivery::new(runtime.identity(), Role::Observer(observer)).unwrap();
        let reference = entry.reference().clone();
        mailboxes.insert_offer(entry).unwrap();
        if !provisional {
            // This actual-source harness owns the public Runtime observer;
            // transfer only its view-book bookkeeping, not a Service grant.
            mailboxes
                .prepare_adoption(runtime.identity(), Role::Observer(observer))
                .unwrap()
                .commit();
        }
        Self {
            runtime,
            source,
            mailboxes,
            observer,
            reference,
            next: 2,
            source_sequence: 0,
        }
    }
    pub fn identity(&self) -> Identity {
        self.runtime.identity()
    }
    pub fn role(&self) -> Role {
        Role::Observer(self.observer)
    }
    pub fn book(&mut self) -> &mut ManagerDelivery {
        let identity = self.identity();
        let role = self.role();
        self.mailboxes.get(identity, role).unwrap()
    }
    pub fn request(&mut self, command: Command) -> Result<Response, RuntimeError> {
        let id = self.next;
        self.next += 1;
        request(&self.runtime, &mut self.mailboxes, id, command)
    }
    pub fn recover(&mut self) -> wire::Recovery {
        let response = self
            .request(Command::ObserverRecoverStamped {
                observer: self.observer,
                capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
            })
            .unwrap();
        self.book().recover(response).unwrap()
    }
    pub fn ack(&mut self, ack: wire::Ack) {
        let reference = self.reference.clone();
        let command = self.book().ack_command(&reference, &ack).unwrap();
        assert!(matches!(
            command,
            Command::ObserverAck { .. } | Command::ObserverLayoutAck { .. }
        ));
        let response = self.request(command).unwrap();
        self.book().commit_ack(&reference, &ack, &response).unwrap();
    }
    pub fn ready(&mut self) -> wire::Recovery {
        let stamp = self.recover();
        self.ack(wire::Ack::Recovery {
            recovery_id: stamp.recovery_id(),
            sequence: stamp.last_sequence(),
            layout_revision: stamp.layout_revision(),
        });
        stamp
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        self.source_sequence += 1;
        self.source
            .try_feed(ExternalChunk {
                sequence: SourceSequence(self.source_sequence),
                stream: ExternalStream::Stdout,
                bytes: bytes.to_vec(),
            })
            .unwrap();
    }
    pub fn finish(&mut self) {
        self.source
            .try_finish(SourceSequence(self.source_sequence))
            .unwrap();
    }
    pub fn drain(&mut self) {
        while let Ok(event) = self.runtime.try_recv() {
            let _ = self.mailboxes.record(self.runtime.identity(), event);
        }
    }
    pub fn events_until(&mut self, condition: impl Fn(&[wire::Event]) -> bool) -> Vec<wire::Event> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.drain();
            let events = self.book().poll(wire::MAX_POLL_DATA_BYTES, 1024).unwrap();
            if condition(&events) {
                return events;
            }
            assert!(Instant::now() < deadline, "actual Runtime event deadline");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub fn retire(&mut self) {
        let identity = self.identity();
        let role = self.role();
        assert!(self.mailboxes.retirements().contains(&(identity, role)));
        let id = RequestId(self.next);
        self.next += 1;
        self.runtime
            .try_send(Request {
                id,
                command: Command::Unobserve {
                    observer: self.observer,
                },
            })
            .unwrap();
        self.mailboxes.sent(id, identity, role);
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.mailboxes.get(identity, role).is_some() {
            self.drain();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
fn request(
    runtime: &Runtime,
    mailboxes: &mut ManagerMailboxes,
    id: u64,
    command: Command,
) -> Result<Response, RuntimeError> {
    runtime
        .try_send(Request {
            id: RequestId(id),
            command,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "actual Runtime reply deadline");
        match runtime.try_recv() {
            Ok(Event::Reply { id: actual, result }) if actual == RequestId(id) => return result,
            Ok(event) => {
                let _ = mailboxes.record(runtime.identity(), event);
            }
            Err(_) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}
