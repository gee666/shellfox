use super::Harness;
use shellfox_runtime::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
#[derive(Default)]
pub(crate) struct Probe {
    pub input: Mutex<Vec<(RequestId, InputKind, Vec<u8>)>>,
    pub output: AtomicUsize,
}
impl LiveOutputSink for Probe {
    fn before_input(
        &self,
        _: Identity,
        id: RequestId,
        kind: InputKind,
        bytes: &[u8],
        _: Instant,
    ) -> Result<(), LiveOutputFailure> {
        let mut input = self.input.lock().unwrap();
        if input.len() >= 32 {
            return Err(LiveOutputFailure);
        }
        input.push((id, kind, bytes.to_vec()));
        Ok(())
    }
    fn output(&self, _: Identity, _: u64, _: &[u8], _: Instant) -> Result<(), LiveOutputFailure> {
        self.output.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
pub(crate) struct Manager {
    pub host: Harness,
    pub events: Vec<Event>,
    pub probe: Arc<Probe>,
}
impl Manager {
    pub fn start(mode: &str, ready: &str) -> Self {
        let probe = Arc::new(Probe::default());
        let mut host = Harness::start_with_hooks(
            mode,
            RuntimeHooks {
                live_output: Some(probe.clone()),
            },
        );
        host.until(|host| String::from_utf8_lossy(&host.output).contains(ready));
        host.request(Command::MouseTracking { lease: host.lease })
            .unwrap();
        Self {
            host,
            events: Vec::new(),
            probe,
        }
    }
    pub fn lease(&self) -> Lease {
        self.host.lease
    }
    pub fn request(&mut self, command: Command) -> Result<Response, RuntimeError> {
        let id = self.host.send(command);
        self.reply(id)
    }
    pub fn reply(&mut self, id: RequestId) -> Result<Response, RuntimeError> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "manager request timed out");
            match self.host.runtime.try_recv() {
                Ok(Event::Reply { id: actual, result }) if actual == id => return result,
                Ok(event) => self.record(event),
                Err(_) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }
    fn record(&mut self, event: Event) {
        assert!(self.events.len() < 1024);
        self.events.push(event);
    }
    pub fn poll_for(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            while let Ok(event) = self.host.runtime.try_recv() {
                self.record(event);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub fn until(&mut self, predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !predicate(self) {
            assert!(
                Instant::now() < deadline,
                "manager event timed out: {:?}",
                self.events
            );
            self.poll_for(Duration::from_millis(2));
        }
    }
    pub fn recover(&mut self) -> StampedRecovery {
        let lease = self.lease();
        match self
            .request(Command::RecoverStamped {
                lease,
                capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
            })
            .unwrap()
        {
            Response::Recovery(snapshot) => snapshot,
            other => panic!("{other:?}"),
        }
    }
    pub fn ack(&mut self, snapshot: &StampedRecovery) -> Result<Response, RuntimeError> {
        let lease = self.lease();
        self.request(Command::ControllerLayoutAck {
            lease,
            recovery_id: snapshot.recovery_id,
            last_delivery_sequence: snapshot.last_delivery_sequence,
            layout_revision: snapshot.layout_revision,
        })
    }
}
pub(crate) fn stamp(snapshot: &StampedRecovery) -> ControllerGeometryStamp {
    ControllerGeometryStamp {
        recovery_id: snapshot.recovery_id,
        layout_revision: snapshot.layout_revision,
        cols: snapshot.cols,
        rows: snapshot.rows,
    }
}
