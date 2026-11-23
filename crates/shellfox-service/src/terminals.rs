use crate::*;
#[path = "terminal_commands.rs"]
mod commands;
#[path = "terminal_delivery.rs"]
mod delivery;
#[path = "terminal_remote_v1.rs"]
mod remote_v1;
pub(crate) use remote_v1::RemoteV1Status;
use serde::{Deserialize, Serialize};
use shellfox_core::{Generation, TabId};
use shellfox_runtime::{Command, Event, Lease, Request, RequestId, Response, Runtime};
use std::{collections::VecDeque, sync::mpsc::TryRecvError};

const MAX_PENDING_BYTES: usize = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub controller: u64,
    pub epoch: u64,
    pub generation: Generation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalOutput {
    pub sequence: u64,
    pub bytes: Vec<u8>,
}
pub(crate) struct OwnedTerminal {
    pub runtime: Runtime,
    pub(crate) startup: Option<std::result::Result<(), shellfox_runtime::RuntimeError>>,
    pub(crate) startup_error_acknowledged: bool,
    pub(crate) external_cleanup: Option<crate::automation::AutomationCleanupProof>,
    pub(crate) control_epoch: u64,
    replies: VecDeque<(
        RequestId,
        std::result::Result<Response, shellfox_runtime::RuntimeError>,
    )>,
    waiting: Option<RequestId>,
    output: VecDeque<TerminalOutput>,
    output_bytes: usize,
    lease: Option<Lease>,
    lease_retiring: bool,
    replay_lost: bool,
    exited: Option<u64>,
    acked: Option<u64>,
    recovery_pending: Option<u64>,
}
impl OwnedTerminal {
    pub(crate) fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            control_epoch: 0,
            startup: None,
            startup_error_acknowledged: false,
            external_cleanup: None,
            replies: VecDeque::new(),
            waiting: None,
            output: VecDeque::new(),
            output_bytes: 0,
            lease: None,
            lease_retiring: false,
            replay_lost: false,
            exited: None,
            acked: None,
            recovery_pending: None,
        }
    }
    pub(crate) fn install_lease(&mut self, lease: Lease) {
        self.discard_output();
        self.lease = Some(lease);
        self.lease_retiring = false;
        self.replay_lost = false;
        self.exited = None;
        self.acked = None;
        self.recovery_pending = None;
    }
    pub(crate) fn retiring_lease(&self) -> Option<Lease> {
        self.lease.filter(|_| self.lease_retiring)
    }
    pub(crate) fn forget_lease(&mut self, lease: Lease) {
        if self.lease == Some(lease) {
            self.lease = None;
            self.lease_retiring = false;
            self.discard_output();
        }
    }
    fn detach(&mut self) {
        self.lease_retiring = self.lease.is_some();
        self.discard_output();
        self.recovery_pending = None;
        self.exited = None;
        self.acked = None;
        self.replay_lost = false;
    }
    pub(crate) fn discard_output(&mut self) {
        self.output.clear();
        self.output_bytes = 0;
    }
}
impl Service {
    pub fn attach(&mut self, tab: TabId, controller: u64, takeover: bool) -> Result<Attachment> {
        let offer = self.attach_with_policy(tab, controller, takeover, true)?;
        self.adopt_controller_grant(offer)
    }
    /// Manager IfFree must reach the actual Runtime unchanged. A missing local
    /// Service slot does not prove the Runtime controller is free.
    pub(crate) fn attach_manager(
        &mut self,
        tab: TabId,
        controller: u64,
        takeover: bool,
    ) -> Result<crate::grant_requests::ControllerGrantOffer> {
        self.attach_with_policy(tab, controller, takeover, false)
    }
    fn attach_with_policy(
        &mut self,
        tab: TabId,
        controller: u64,
        takeover: bool,
        legacy_detached_takeover: bool,
    ) -> Result<crate::grant_requests::ControllerGrantOffer> {
        if controller == 0 {
            bail!("Controller must not be zero.");
        }
        if !self.terminals.contains_key(&tab) {
            self.start_terminal(tab)?;
        }
        let detached = self.terminals[&tab].lease.is_none() || self.terminals[&tab].lease_retiring;
        self.controller_grant(
            tab,
            controller,
            takeover || (legacy_detached_takeover && detached),
        )
        .map_err(Into::into)
    }
    pub(crate) fn lease(&self, tab: TabId, attachment: &Attachment) -> Result<Lease> {
        let terminal = self
            .terminals
            .get(&tab)
            .context("Terminal is not running.")?;
        if terminal.runtime.identity().generation != attachment.generation {
            bail!("Terminal generation is stale.");
        }
        let lease = Lease {
            controller: attachment.controller,
            epoch: attachment.epoch,
        };
        if terminal.lease_retiring || terminal.lease != Some(lease) {
            bail!("Terminal control lease is detached or stale.");
        }
        Ok(lease)
    }
    /// Revoke only this controller's service authority, never its process. The
    /// next authorized attach may claim a fresh runtime epoch without takeover.
    pub(crate) fn detach_attachment(&mut self, tab: TabId, attachment: &Attachment) -> Result<()> {
        self.lease(tab, attachment)?;
        self.terminals
            .get_mut(&tab)
            .context("Terminal is not running.")?
            .detach();
        Ok(())
    }
    pub fn detach_controller(&mut self, controller: u64) {
        self.activity.detach_desktop_visibility(controller);
        self.release_automation_controller(controller);
        self.revoke_system_native(controller);
        self.revoke_native_catalog(controller);
        for terminal in self.terminals.values_mut() {
            if terminal
                .lease
                .is_some_and(|lease| lease.controller == controller)
            {
                terminal.detach();
            }
        }
    }
    /// Existing private CLI protocol is explicitly Legacy until its DTO carries
    /// captured settlement. Missing native contexts never enter this method.
    pub fn input(&mut self, tab: TabId, attachment: &Attachment, bytes: Vec<u8>) -> Result<()> {
        self.input_with_context(tab, attachment, bytes, true, shellfox_host::CapturedSettlement::Legacy)
    }
    /// A CLI key, encoded by Ghostty with the terminal's live keyboard modes.
    pub fn key(&mut self, tab: TabId, attachment: &Attachment, event: shellfox_ghostty_vt::key::KeyEvent) -> Result<()> {
        let lease = self.lease(tab, attachment)?;
        let user_initiated = event.action != shellfox_ghostty_vt::key::KeyAction::Release;
        let admission = crate::activity_integration::InputAdmission::native(user_initiated, false, shellfox_host::CapturedSettlement::Legacy);
        let response = self.call_input(tab, Command::Key { lease, event }, admission)?;
        if !matches!(response, Response::Input(receipt) if matches!(receipt.kind, shellfox_runtime::InputKind::Key { .. })) {
            bail!("Runtime key acceptance was not confirmed; outcome is uncertain and input was not retried.");
        }
        Ok(())
    }
    /// A CLI paste, framed per the application's bracketed-paste mode.
    pub fn paste(&mut self, tab: TabId, attachment: &Attachment, text: String) -> Result<()> {
        let lease = self.lease(tab, attachment)?;
        let admission = crate::activity_integration::InputAdmission::native(!text.is_empty(), false, shellfox_host::CapturedSettlement::Legacy);
        let response = self.call_input(tab, Command::Paste { lease, text }, admission)?;
        if !matches!(response, Response::Input(receipt) if matches!(receipt.kind, shellfox_runtime::InputKind::Paste { .. })) {
            bail!("Runtime paste acceptance was not confirmed; outcome is uncertain and input was not retried.");
        }
        Ok(())
    }
    fn input_with_context(&mut self, tab: TabId, attachment: &Attachment, bytes: Vec<u8>, user_initiated: bool, settlement: shellfox_host::CapturedSettlement) -> Result<()> {
        let lease = self.lease(tab, attachment)?;
        let length = u32::try_from(bytes.len())?;
        let admission = crate::activity_integration::InputAdmission::raw(
            &bytes,
            user_initiated,
            false,
            settlement,
        )
        .map_err(|_| anyhow::anyhow!("Invalid bounded CLI input admission."))?;
        let response = self.call_input(tab, Command::RawInput { lease, bytes }, admission)?;
        if !matches!(response, Response::Input(receipt) if receipt.kind == shellfox_runtime::InputKind::Raw && receipt.accepted_bytes == length)
        {
            bail!("Runtime input acceptance was not confirmed; outcome is uncertain and input was not retried.");
        }
        Ok(())
    }
    pub fn resize(
        &mut self,
        tab: TabId,
        attachment: &Attachment,
        cols: u16,
        rows: u16,
    ) -> Result<()> {
        let lease = self.lease(tab, attachment)?;
        self.call(tab, Command::Resize { lease, cols, rows })?;
        Ok(())
    }
    pub fn recover(
        &mut self,
        tab: TabId,
        attachment: &Attachment,
    ) -> Result<(Vec<u8>, u64, shellfox_runtime::AnsiRecoveryCapability)> {
        let lease = self.lease(tab, attachment)?;
        let Response::Ansi { bytes, last_sequence, capability } = self.call(tab, Command::RecoverAnsi { lease })? else {
            bail!("Unexpected runtime recovery response.");
        };
        self.install_recovery_cut(tab, last_sequence)?;
        Ok((bytes, last_sequence, capability))
    }
    fn install_recovery_cut(&mut self, tab: TabId, last_sequence: u64) -> Result<()> {
        self.drain(tab)?;
        let terminal = self.terminals.get_mut(&tab).context("Terminal is not running.")?;
        terminal.output.retain(|chunk| chunk.sequence > last_sequence);
        terminal.output_bytes = terminal.output.iter().map(|chunk| chunk.bytes.len()).sum();
        terminal.replay_lost = false;
        terminal.recovery_pending = Some(last_sequence);
        Ok(())
    }
    /// Disconnecting clients do not invoke RetryClose. Host terminals survive detach.
    pub fn poll(&mut self, tab: TabId, attachment: &Attachment) -> Result<Vec<TerminalOutput>> {
        let lease = self.lease(tab, attachment)?;
        // Validate the epoch even when there is no output waiting.
        self.call(tab, Command::MouseTracking { lease })?;
        self.drain(tab)?;
        let terminal = self
            .terminals
            .get(&tab)
            .context("Terminal is not running.")?;
        Ok(terminal.output.iter().cloned().collect())
    }
    /// Client ACKs only after it writes/consumes the bytes. Failed replies remain
    /// available to poll again. A new claim requires explicit screen recovery.
    pub fn acknowledge(
        &mut self,
        tab: TabId,
        attachment: &Attachment,
        sequence: u64,
    ) -> Result<()> {
        let lease = self.lease(tab, attachment)?;
        if self.terminals[&tab]
            .recovery_pending
            .is_some_and(|barrier| barrier != sequence)
        {
            bail!("Acknowledge the recovery barrier before newer output.");
        }
        self.call(tab, Command::Ack { lease, sequence })?;
        let terminal = self
            .terminals
            .get_mut(&tab)
            .context("Terminal is not running.")?;
        terminal.acked = Some(sequence);
        terminal.recovery_pending = None;
        terminal.output.retain(|chunk| chunk.sequence > sequence);
        terminal.output_bytes = terminal.output.iter().map(|chunk| chunk.bytes.len()).sum();
        Ok(())
    }
    pub(crate) fn poll_status(&self, tab: TabId) -> Result<(bool, Option<u64>)> {
        let terminal = self
            .terminals
            .get(&tab)
            .context("Terminal owner missing.")?;
        Ok((
            terminal.replay_lost,
            terminal.exited.filter(|last| {
                terminal.output.is_empty()
                    && terminal.recovery_pending.is_none()
                    && terminal.acked.is_some_and(|acked| acked >= *last)
            }),
        ))
    }
}
