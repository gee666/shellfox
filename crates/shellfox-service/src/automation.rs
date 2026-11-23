//! One in-memory owner of local automation processes. No output/argv/env store.
mod launch;
mod lifecycle;
mod relay;
mod start;
pub use relay::AutomationRelay;
mod failure;
mod retirement;
pub use failure::{
    AutomationFailedCleanupProof, AutomationFailedSourceEvidence, AutomationSourceAbortReason,
    AutomationSourceFailure, AutomationSourceFailureKind,
};
pub use retirement::{
    AutomationCleanupProof, AutomationPublication, AutomationRetirement, AutomationRetirementAck,
};
#[cfg(test)]
mod tests;
mod view;
use anyhow::{ensure, Context, Result};
use shellfox_protocol::{automation::*, Validate};
pub use shellfox_terminal::raw_process::AutomationStdinClosed;
use shellfox_terminal::raw_process::{RawCommand, RawOutput, RawProcess, RawStream, WriteReceipt};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;
pub use view::{AutomationView, AutomationViewEvent, AutomationViewer};

pub struct AutomationRuns {
    supervisor: PathBuf,
    runs: BTreeMap<Uuid, Run>,
    failed: Vec<FailedProcess>,
    prepared: BTreeMap<Uuid, Prepared>,
    retired: BTreeMap<Uuid, AutomationRetirement>,
    closing: bool,
}
struct Run {
    input: AutomationStart,
    owner: u64,
    process: Option<RawProcess>,
    dto: AutomationRun,
    frames: VecDeque<StoredFrame>,
    bytes: usize,
    sequence: u64,
    acknowledged: u64,
    capturing: bool,
    last_read: Instant,
    input_ended: bool,
    stopping: bool,
    eof: [bool; 2],
    outcome: Option<i32>,
    forced_stop: bool,
    source: Option<AutomationView>,
    source_identity: Option<shellfox_runtime::Identity>,
    publisher: view::Publisher,
    pending: Option<AutomationViewEvent>,
    source_failure: Option<AutomationSourceFailure>,
    failed_cleanup: Option<AutomationFailedCleanupProof>,
    launch_evidence: Option<shellfox_terminal::NativeLaunchEvidence>,
}
struct Prepared {
    input: AutomationStart,
    owner: u64,
    source: Option<AutomationView>,
    source_identity: Option<shellfox_runtime::Identity>,
    publisher: view::Publisher,
}
struct FailedProcess {
    id: Uuid,
    process: RawProcess,
    _publisher: view::Publisher,
}
const MAX_PIPE_FRAMES: usize = 1024;
struct StoredFrame {
    frame: AutomationFrame,
    bytes: usize,
}
impl AutomationRuns {
    pub fn new(supervisor: PathBuf) -> Self {
        Self {
            supervisor,
            runs: BTreeMap::new(),
            failed: vec![],
            prepared: BTreeMap::new(),
            retired: BTreeMap::new(),
            closing: false,
        }
    }
    pub fn list(&mut self) -> Result<AutomationListing> {
        self.pump()?;
        Ok(AutomationListing(
            self.runs.values().map(|run| run.dto.clone()).collect(),
        ))
    }
    fn owned(&mut self, id: Uuid, owner: u64) -> Result<&mut Run> {
        let run = self
            .runs
            .get_mut(&id)
            .context("Automation run not found.")?;
        ensure!(
            owner > 0 && run.owner == owner,
            "Only the starting client can use this run's pipes."
        );
        Ok(run)
    }
    pub fn read(&mut self, request: AutomationRead, owner: u64) -> Result<AutomationOutput> {
        request.validate()?;
        self.owned(request.id, owner)?;
        self.pump()?;
        let run = self.owned(request.id, owner)?;
        ensure!(run.capturing || run.stopping, "Pipe collection ended. The process continues; use runs show to inspect its bounded visual output.");
        ensure!(
            request.after >= run.acknowledged && request.after <= run.sequence,
            "Invalid automation output cursor."
        );
        run.last_read = Instant::now();
        run.acknowledged = request.after;
        while run
            .frames
            .front()
            .is_some_and(|f| f.frame.sequence <= request.after)
        {
            if let Some(frame) = run.frames.pop_front() {
                run.bytes -= frame.bytes;
            }
        }
        Ok(AutomationOutput {
            frames: run
                .frames
                .iter()
                .take(16)
                .map(|f| f.frame.clone())
                .collect(),
            run: run.dto.clone(),
        })
    }
    /// Returns an actual OS-write receipt, not a queue-admission ACK. The local
    /// owner must defer its reply and poll this without blocking the Service.
    pub fn begin_write(&mut self, request: AutomationWrite, owner: u64) -> Result<WriteReceipt> {
        request.validate()?;
        let run = self.owned(request.id, owner)?;
        if run.input_ended || run.dto.state != AutomationState::Running {
            return Err(AutomationStdinClosed::known().into());
        }
        let bytes = shellfox_protocol::decode_chunk(&request.data)?;
        let receipt = run
            .process
            .as_mut()
            .context("Automation process owner is unavailable.")?
            .write(bytes, request.eof)?;
        if request.eof {
            run.input_ended = true;
        }
        Ok(receipt)
    }
    /// Service takes this once immediately after Start and starts the single
    /// external-source Runtime BEFORE returning Start. Never defer this to Show.
    pub fn take_source(&mut self, id: Uuid) -> Result<AutomationView> {
        let source = if let Some(prepared) = self.prepared.get_mut(&id) {
            &mut prepared.source
        } else {
            &mut self
                .runs
                .get_mut(&id)
                .context("Automation run not found.")?
                .source
        };
        source
            .take()
            .context("Automation source already has an authoritative consumer.")
    }
    pub fn show(
        &mut self,
        request: AutomationShow,
        viewer: &mut dyn AutomationViewer,
    ) -> Result<AutomationRun> {
        self.pump()?;
        let run = self
            .runs
            .get_mut(&request.id)
            .context("Automation run not found.")?;
        if run.dto.session_id.is_none() {
            ensure!(
                run.source.is_none(),
                "Automation authoritative screen was not started."
            );
            let identity = run
                .source_identity
                .context("Automation canonical Runtime identity was not bound.")?;
            let (session, tab) = viewer.create_read_only(&run.dto)?;
            ensure!(
                identity.session_id.to_string() == session.to_string()
                    && identity.tab_id.to_string() == tab.to_string(),
                "Automation publication changed canonical Runtime identity."
            );
            run.dto.session_id = Some(session);
            run.dto.tab_id = Some(tab);
        }
        viewer.focus(
            run.dto
                .session_id
                .context("Automation viewer did not create a session.")?,
        )?;
        Ok(run.dto.clone())
    }
}
// Node exposes Windows DWORDs as positive numbers; std ExitStatus uses i32
// bits. Unix retains its own signed/signal conventions rather than this cast.
fn compatible_natural_exit(code: i32) -> i64 {
    #[cfg(windows)]
    {
        i64::from(code as u32)
    }
    #[cfg(not(windows))]
    {
        i64::from(code)
    }
}
impl Run {
    fn finalize_dto(&mut self, code: i32) {
        self.dto.state = AutomationState::Exited;
        self.input_ended = true;
        // Legacy local API convention, not OS-signal evidence or cleanup proof.
        self.dto.exit_code = if self.forced_stop {
            None
        } else {
            Some(compatible_natural_exit(code))
        };
        self.dto.signal = self.forced_stop.then(|| "SIGKILL".to_owned());
    }
    fn release(&mut self) {
        self.capturing = false;
        self.input_ended = true;
        self.frames.clear();
        self.bytes = 0;
        if let Some(process) = &mut self.process {
            process.release_stdin();
        }
    }
    fn accept_pending(&mut self) -> Result<bool> {
        let Some(event) = self.pending.take() else {
            return Ok(true);
        };
        let mut frame = None;
        match &event {
            AutomationViewEvent::Data {
                sequence,
                stream,
                bytes,
            } => {
                if self.capturing
                    && (self.bytes + bytes.len() > AUTOMATION_CREDIT
                        || self.frames.len() >= MAX_PIPE_FRAMES)
                {
                    self.pending = Some(event);
                    return Ok(false);
                }
                if self.capturing {
                    frame = Some(StoredFrame {
                        bytes: bytes.len(),
                        frame: AutomationFrame {
                            sequence: *sequence,
                            stream: stream.clone(),
                            data: shellfox_protocol::encode_chunk(bytes)?,
                        },
                    });
                }
            }
            AutomationViewEvent::Eof { .. } | AutomationViewEvent::Exit { .. } => {}
        }
        let sequence = match &event {
            AutomationViewEvent::Data { sequence, .. } => *sequence,
            _ => self.sequence,
        };
        let eof = match &event {
            AutomationViewEvent::Eof { stream, .. } => {
                Some(usize::from(*stream == AutomationStream::Stderr))
            }
            _ => None,
        };
        let exit = match &event {
            AutomationViewEvent::Exit { code, .. } => Some(*code),
            _ => None,
        };
        if let Err(event) = self.publisher.try_push(event) {
            self.pending = Some(event);
            return Ok(false);
        }
        self.sequence = sequence;
        if let Some(frame) = frame {
            self.bytes += frame.bytes;
            self.frames.push_back(frame);
        }
        if let Some(index) = eof {
            self.eof[index] = true;
        }
        if let Some(code) = exit {
            self.finalize_dto(code);
        }
        Ok(true)
    }
}
