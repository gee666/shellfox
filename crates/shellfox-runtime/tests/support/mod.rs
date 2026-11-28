#![cfg(windows)]
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::*;
use shellfox_terminal::{PtyCommand, PtyConfig};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
pub(super) struct Harness {
    pub(super) runtime: Runtime,
    root: PathBuf,
    pub(super) lease: Lease,
    pub(super) next: u64,
    pub(super) output: Vec<u8>,
    pub(super) exited: bool,
    pub(super) last_screen: Option<ScreenFrame>,
    pub(super) observer_output: Vec<(ObserverLease, Chunk)>,
    pub(super) observer_exits: Vec<ObserverLease>,
}
impl Harness {
    pub(super) fn start(mode: &str) -> Self {
        Self::start_with_hooks(mode, RuntimeHooks::default())
    }
    pub(super) fn start_with_hooks(mode: &str, hooks: RuntimeHooks) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("runtime-test-{}", Generation::new()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-runtime-fixture"));
        let mut config = PtyConfig::isolated(&root);
        config.supervisor = Some(executable.clone());
        config.command = Some(PtyCommand {
            program: executable.into_os_string(),
            args: vec![mode.into()],
        });
        let runtime = Runtime::start_with_hooks(
            Identity {
                session_id: SessionId::new(),
                tab_id: TabId::new(),
                generation: Generation::new(),
            },
            config,
            hooks,
        )
        .unwrap();
        let mut this = Self {
            runtime,
            root,
            lease: Lease {
                controller: 0,
                epoch: 0,
            },
            next: 1,
            output: Vec::new(),
            exited: false,
            last_screen: None,
            observer_output: Vec::new(),
            observer_exits: Vec::new(),
        };
        this.lease = match this
            .request(Command::Claim {
                controller: 1,
                takeover: false,
            })
            .unwrap()
        {
            Response::Claimed(lease) => lease,
            response => panic!("unexpected claim: {response:?}"),
        };
        let response = this
            .request(Command::Replay {
                lease: this.lease,
                after: 0,
            })
            .unwrap();
        if let Response::Replay(chunks) = response {
            let last = chunks.last().map(|c| c.sequence);
            for chunk in chunks {
                this.output.extend(chunk.bytes);
            }
            if let Some(sequence) = last {
                this.ack(sequence);
            }
        } else {
            panic!("expected replay");
        }
        this
    }
    pub(super) fn send(&mut self, command: Command) -> RequestId {
        let id = RequestId(self.next);
        self.next += 1;
        // The command queue is bounded and a burst of output acks can fill it;
        // the owner drains it on its own thread, so waiting is enough.
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut request = Request { id, command };
        loop {
            match self.runtime.try_send(request) {
                Ok(()) => return id,
                Err(SubmitError::Full(rejected)) if Instant::now() < deadline => {
                    request = rejected;
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("command not sent: {error:?}"),
            }
        }
    }
    fn ack(&mut self, sequence: u64) {
        self.send(Command::Ack {
            lease: self.lease,
            sequence,
        });
    }
    pub(super) fn observe(&mut self, event: Event) {
        if let Some(screen) = self.runtime.take_screen() {
            self.last_screen = Some(screen);
        }
        match event {
            Event::Started(Err(error)) => panic!("startup failed: {error}"),
            Event::Output { lease, chunk } if lease == self.lease => {
                self.output.extend(chunk.bytes);
                self.ack(chunk.sequence);
            }
            Event::Exited { .. } => self.exited = true,
            Event::ObserverOutput { observer, chunk } => {
                self.observer_output.push((observer, chunk))
            }
            Event::ObserverExited { observer, .. } => self.observer_exits.push(observer),
            Event::ObserverReplayLost { .. } => panic!("unexpected observer replay loss"),
            Event::ReplayLost { .. } => panic!("unexpected replay loss"),
            Event::Reply {
                result: Err(error), ..
            } => panic!("unexpected asynchronous error: {error}"),
            _ => {}
        }
    }
    pub(super) fn request(&mut self, command: Command) -> Result<Response, RuntimeError> {
        let id = self.send(command);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                Instant::now() < deadline,
                "request timed out, phase {:?}",
                self.runtime.phase()
            );
            match self.runtime.try_recv() {
                Ok(Event::Reply {
                    id: received,
                    result,
                }) if received == id => return result,
                Ok(event) => self.observe(event),
                Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }
    pub(super) fn until(&mut self, predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !predicate(self) {
            assert!(
                Instant::now() < deadline,
                "runtime timed out, phase {:?}, bytes {:?}",
                self.runtime.phase(),
                String::from_utf8_lossy(&self.output)
            );
            match self.runtime.try_recv() {
                Ok(event) => self.observe(event),
                Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
            if let Some(screen) = self.runtime.take_screen() {
                self.last_screen = Some(screen);
            }
        }
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.runtime.try_send(Request {
            id: RequestId(u64::MAX),
            command: Command::RetryClose,
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.runtime.phase() != Phase::Closed && Instant::now() < deadline {
            while self.runtime.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(2));
        }
        if self.runtime.phase() == Phase::Closed {
            let _ = std::fs::remove_dir_all(&self.root);
        } else {
            eprintln!(
                "cleanup unconfirmed; retaining scratch {}",
                self.root.display()
            );
        }
    }
}
