//! Local desktop/headless terminal owner. No GUI, transport, or metadata writes.
//! Mobile bindings must not depend on this crate.
mod ansi;
mod api;
mod commands;
mod delivery;
mod event_api;
pub use event_api::*;
mod controller_commands;
mod external;
mod external_cleanup;
pub use external_cleanup::{
    poll_external_cleanup, ExternalCleanupReport, ExternalStartError, ExternalStartStage,
    ExternalStartupOwner,
};
mod external_display;
mod external_owner;
pub use external_display::ExternalDisplayPolicy;
mod external_runtime;
pub use external::{
    DeliveryRange, DeliverySequence, ExternalAbortEvidence, ExternalAbortReason, ExternalChunk,
    ExternalConfig, ExternalFeedError, ExternalFinishError, ExternalSource, ExternalStatus,
    ExternalStream, SourceKind, SourceSequence, EXTERNAL_CHUNK_BYTES, EXTERNAL_FEED_CAPACITY,
};
#[cfg(test)]
mod handle_tests;
mod hooks;
pub use hooks::{LiveOutputFailure, LiveOutputSink, LiveOutputStatus, RuntimeHooks};
mod input_receipt;
mod inspection;
pub use input_receipt::{InputKind, InputReceipt};
mod launch;
pub use shellfox_terminal::launch_privilege::{
    LaunchEvidenceBasis, LaunchEvidenceStage, LaunchFactoryPurpose, LaunchPrivilege,
    LaunchPrivilegeFacts, LaunchUnknownReason, PosixLaunchFacts, WindowsElevationType,
    WindowsLaunchToken,
};
mod layout;
mod layout_api;
mod metadata;
mod stamped_recovery;
pub use inspection::{InspectionFailure, InspectionPoll, InspectionTicket, ProcessInspection};
pub use layout_api::{
    ControllerGeometryStamp, ControllerRevocation, LayoutStamp, RecoveryCut, RecoveryId,
    StampedRecovery,
};
pub use metadata::RuntimeMetadata;
mod observer_commands;
mod observers;
mod output_read;
pub use output_read::{OutputRead, ReadOutput};
mod owner;
mod owner_events;
mod presentation;
mod presentation_api;
mod presentation_commands;
pub use presentation_api::*;
mod owner_wake;
pub use owner_wake::EventWake;
mod pty_output;
#[cfg(test)]
mod query_tests;
mod recovery;
mod source_feed;

pub use api::*;
use shellfox_terminal::PtyConfig;
pub use shellfox_terminal::UnixPrivilegeMode;
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    Arc, Mutex,
};

pub type StartError = std::io::Error;
#[derive(Debug)]
pub enum SubmitError {
    Invalid(RuntimeError),
    Full(Request),
    Disconnected(Request),
}

/// One owner thread creates both the !Send parser and the owned PTY.
/// Dropping this handle requests cleanup even when the command queue is full.
/// Failed cleanup stays owned by that thread, not by a detached numeric PID.
pub struct Runtime {
    identity: Identity,
    limits: GridLimits,
    launch: Arc<launch::LaunchState>,
    commands: SyncSender<Request>,
    events: Receiver<Event>,
    shared: Arc<Shared>,
    inspection: Option<inspection::InspectionClient>,
    source_kind: SourceKind,
    external_source: Option<ExternalSource>,
    external_status: Option<Arc<Mutex<ExternalStatus>>>,
    external_control: Option<external_cleanup::ExternalControl>,
}
pub(crate) struct Shared {
    screen: Mutex<Option<Arc<ScreenFrame>>>,
    legacy_requested: AtomicBool,
    live_output_status: AtomicU8,
    metadata: Mutex<Option<Arc<RuntimeMetadata>>>,
    phase: Mutex<Phase>,
    abandoned: AtomicBool,
    /// Wakes the owner thread for commands, PTY output, exit and released room.
    signal: Arc<owner_wake::OwnerSignal>,
    event_wake: Mutex<Option<EventWake>>,
    /// The owner found the event channel full and waits for the consumer.
    needs_space: AtomicBool,
}
impl Shared {
    fn new(live_output_status: LiveOutputStatus, phase: Phase) -> Self {
        Self {
            screen: Mutex::new(None),
            legacy_requested: AtomicBool::new(false),
            live_output_status: AtomicU8::new(live_output_status as u8),
            metadata: Mutex::new(None),
            phase: Mutex::new(phase),
            abandoned: AtomicBool::new(false),
            signal: Arc::default(),
            event_wake: Mutex::new(None),
            needs_space: AtomicBool::new(false),
        }
    }
}
impl Runtime {
    /// The caller must register identity with Host before calling start.
    /// A spawn error arrives as Started(Err). Keep this handle until phase Closed.
    pub fn start(identity: Identity, config: PtyConfig) -> Result<Self, StartError> {
        Self::start_with_limits(identity, config, GridLimits::default())
    }
    pub fn start_with_limits(
        identity: Identity,
        config: PtyConfig,
        limits: GridLimits,
    ) -> Result<Self, StartError> {
        Self::start_with_limits_and_hooks(identity, config, limits, RuntimeHooks::default())
    }
    pub fn start_with_hooks(
        identity: Identity,
        config: PtyConfig,
        hooks: RuntimeHooks,
    ) -> Result<Self, StartError> {
        Self::start_with_limits_and_hooks(identity, config, GridLimits::default(), hooks)
    }
    pub fn start_with_limits_and_hooks(
        identity: Identity,
        config: PtyConfig,
        limits: GridLimits,
        hooks: RuntimeHooks,
    ) -> Result<Self, StartError> {
        Self::start_owned(identity, config, limits, hooks)
    }
    #[cfg(windows)]
    pub fn start_administrator(
        identity: Identity,
        config: PtyConfig,
        helper: shellfox_terminal::elevated::InteractiveHelper,
        hooks: RuntimeHooks,
    ) -> Result<Self, StartError> {
        Self::start_owned_configured(identity, config, GridLimits::default(), hooks, Some(helper))
    }
    fn start_owned(
        identity: Identity,
        config: PtyConfig,
        limits: GridLimits,
        hooks: RuntimeHooks,
    ) -> Result<Self, StartError> {
        Self::start_owned_configured(
            identity,
            config,
            limits,
            hooks,
            #[cfg(windows)]
            None,
        )
    }
    fn start_owned_configured(
        identity: Identity,
        config: PtyConfig,
        limits: GridLimits,
        hooks: RuntimeHooks,
        #[cfg(windows)] administrator: Option<shellfox_terminal::elevated::InteractiveHelper>,
    ) -> Result<Self, StartError> {
        limits
            .validate_size(config.cols, config.rows)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let (commands, incoming) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (outgoing, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let shared = Arc::new(Shared::new(hooks.status(), Phase::Starting));
        let (inspection_client, inspection_publisher) = inspection::start(identity)?;
        let (launch, launch_binding) = launch::new(LaunchFactoryPurpose::OwnedPtyProgram);
        let launch_owner = Arc::clone(&launch);
        let owner_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("shellfox-terminal-owner".into())
            .spawn(move || {
                owner::run(
                    identity,
                    config,
                    limits,
                    incoming,
                    outgoing,
                    owner_shared,
                    owner::OwnerServices {
                        inspection: Some(inspection_publisher),
                        hooks,
                        #[cfg(windows)]
                        administrator,
                        launch: launch_owner,
                        launch_binding,
                    },
                )
            })?;
        Ok(Self {
            identity,
            limits,
            launch,
            commands,
            events,
            shared,
            inspection: Some(inspection_client),
            source_kind: SourceKind::OwnedPty,
            external_source: None,
            external_status: None,
            external_control: None,
        })
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
    pub fn grid_limits(&self) -> GridLimits {
        self.limits
    }
    pub fn try_send(&self, request: Request) -> Result<(), SubmitError> {
        request
            .command
            .validate(self.limits)
            .map_err(SubmitError::Invalid)?;
        self.commands
            .try_send(request)
            .map_err(|error| match error {
                TrySendError::Full(request) => SubmitError::Full(request),
                TrySendError::Disconnected(request) => SubmitError::Disconnected(request),
            })?;
        self.shared.signal.notify();
        Ok(())
    }
    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        let event = self.events.try_recv()?;
        self.shared.event_taken();
        Ok(event)
    }
    /// Called from the owner thread after it queues events for try_recv, at
    /// most once per owner round. Called once now for events already queued.
    /// The callback must not block.
    pub fn set_event_wake(&self, wake: EventWake) {
        *self
            .shared
            .event_wake
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Arc::clone(&wake));
        wake();
    }
    /// At most one owned UTF-8 cell frame waits here. A slow UI replaces frames,
    /// never queues them. Request Screen to publish an unchanged screen again.
    pub fn take_screen(&self) -> Option<ScreenFrame> {
        if !self.shared.legacy_requested.swap(true, Ordering::AcqRel) {
            self.shared.signal.notify();
        }
        self.shared
            .screen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(Arc::unwrap_or_clone)
    }
    pub fn live_output_status(&self) -> LiveOutputStatus {
        hooks::status(&self.shared.live_output_status)
    }
    pub fn metadata(&self) -> Option<Arc<RuntimeMetadata>> {
        self.shared
            .metadata
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    pub fn request_inspection(&self) -> Result<InspectionTicket, InspectionFailure> {
        self.inspection
            .as_ref()
            .ok_or(InspectionFailure::Unsupported)?
            .request()
    }
    /// Read-only owned-process capture for an explicit request such as session
    /// stats. Capture it off the owner thread; None when this terminal has no inspection.
    pub fn inspection_capability(
        &self,
    ) -> Option<shellfox_terminal::inspection::InspectionCapability> {
        self.inspection.as_ref()?.capability()
    }
    pub fn poll_inspection(&self, ticket: InspectionTicket) -> InspectionPoll {
        match &self.inspection {
            Some(client) => client.poll(ticket),
            None => InspectionPoll::Ready(Err(InspectionFailure::Unsupported)),
        }
    }
    pub fn phase(&self) -> Phase {
        *self.shared.phase.lock().unwrap_or_else(|e| e.into_inner())
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(client) = &self.inspection {
            client.invalidate();
        }
        self.shared.abandon();
    }
}
