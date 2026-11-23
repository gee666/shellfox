#![cfg(windows)]
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_host::CapturedSettlement;
use shellfox_runtime::{
    Command, Event, Identity, Phase, Request, RequestId, Response, Runtime, RuntimeHooks,
};
use shellfox_service::activity::integration::{
    AcceptedInputContext, InputAdmission, InspectionScope, LiveActivitySink, OwnerActivity,
};
use shellfox_terminal::{PtyCommand, PtyConfig};
use std::sync::Arc;
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Fixture {
    pub runtimes: HashMap<TabId, Runtime>,
    pub identity: Identity,
    pub sink: Arc<LiveActivitySink>,
    scratch: Option<tempfile::TempDir>,
    pub output: Vec<u8>,
    next: u64,
}
impl Fixture {
    pub fn start(mode: &str) -> (Self, PathBuf, OwnerActivity) {
        Self::start_with_identity(
            mode,
            Identity {
                session_id: SessionId::new(),
                tab_id: TabId::new(),
                generation: Generation::new(),
            },
        )
    }
    pub fn start_with_identity(mode: &str, identity: Identity) -> (Self, PathBuf, OwnerActivity) {
        let profile = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        let executable = profile.join("shellfox-runtime-fixture.exe");
        assert!(
            executable.is_file(),
            "build shellfox-runtime-fixture before this integration test"
        );
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&root).unwrap();
        let scratch = tempfile::Builder::new()
            .prefix("activity-native-")
            .tempdir_in(root)
            .unwrap();
        let mut config = PtyConfig::new(std::fs::canonicalize(scratch.path()).unwrap());
        config.supervisor = Some(executable.clone());
        config.command = Some(PtyCommand {
            program: executable.clone().into_os_string(),
            args: vec![mode.into()],
        });
        let mut policy = OwnerActivity::new();
        policy
            .watch(
                identity,
                executable.to_string_lossy().into_owned(),
                InspectionScope::LocalWindows,
            )
            .unwrap();
        let sink = policy.live_output_sink(identity).unwrap();
        let hooks = RuntimeHooks {
            live_output: Some(sink.clone()),
        };
        let runtime = Runtime::start_with_hooks(identity, config, hooks).unwrap();
        (
            Self {
                runtimes: HashMap::from([(identity.tab_id, runtime)]),
                identity,
                sink,
                scratch: Some(scratch),
                output: Vec::new(),
                next: 1,
            },
            executable,
            policy,
        )
    }
    pub fn runtime(&self) -> &Runtime {
        &self.runtimes[&self.identity.tab_id]
    }
    pub fn request(&mut self, command: Command) -> Response {
        let admission = match &command {
            Command::RawInput { bytes, .. } => {
                Some(InputAdmission::raw(bytes, true, true, CapturedSettlement::Legacy).unwrap())
            }
            Command::Key { .. } | Command::Paste { .. } | Command::Mouse { .. } => Some(
                InputAdmission::native(true, true, CapturedSettlement::Legacy),
            ),
            _ => None,
        };
        self.request_with_context(command, admission).0
    }
    pub fn request_with_context(
        &mut self,
        command: Command,
        admission: Option<InputAdmission>,
    ) -> (Response, Option<AcceptedInputContext>) {
        let id = self.next;
        self.next += 1;
        if let Some(admission) = admission {
            self.sink
                .register_input(self.identity, RequestId(id), admission)
                .unwrap();
        }
        if self
            .runtime()
            .try_send(Request {
                id: RequestId(id),
                command,
            })
            .is_err()
        {
            self.sink.cancel_input(self.identity, RequestId(id));
            panic!("controlled runtime submission failed");
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "owned runtime request timed out");
            match self.runtime().try_recv() {
                Ok(Event::Reply { id: got, result }) if got.0 == id => {
                    let captured = self.sink.finish_input(self.identity, got, &result).unwrap();
                    return (result.unwrap(), captured);
                }
                Ok(event) => self.event(event),
                Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }
    fn event(&mut self, event: Event) {
        match event {
            Event::Started(Err(error)) => panic!("controlled fixture startup failed: {error}"),
            Event::Output { chunk, .. } => self.output.extend(chunk.bytes),
            _ => {}
        }
    }
    pub fn until_output(&mut self, marker: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !String::from_utf8_lossy(&self.output).contains(marker) {
            assert!(
                Instant::now() < deadline,
                "controlled fixture output timed out"
            );
            match self.runtime().try_recv() {
                Ok(event) => self.event(event),
                Err(_) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }
    pub fn close(&mut self) {
        if self.runtimes.is_empty() {
            return;
        }
        self.runtime()
            .try_send(Request {
                id: RequestId(u64::MAX),
                command: Command::RetryClose,
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.runtime().phase() != Phase::Closed {
            assert!(
                Instant::now() < deadline,
                "owned Job cleanup was not confirmed"
            );
            while self.runtime().try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(2));
        }
        self.sink.cancel_all_inputs(self.identity);
        self.runtimes.clear();
        if let Some(scratch) = self.scratch.take() {
            scratch.close().unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.runtimes.is_empty() {
            return;
        }
        let _ = self.runtime().try_send(Request {
            id: RequestId(u64::MAX),
            command: Command::RetryClose,
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.runtime().phase() != Phase::Closed && Instant::now() < deadline {
            while self.runtime().try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(2));
        }
        if self.runtime().phase() != Phase::Closed {
            if let Some(scratch) = self.scratch.take() {
                let _ = scratch.keep();
            }
        }
    }
}
