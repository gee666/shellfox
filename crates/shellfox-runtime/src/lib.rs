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
mod key_v2;
pub use key_v2::{InvalidKeyV2, KeyActionV2, ModifiersV2, OwnedKeyEventV2, PhysicalKey};
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
#[cfg(feature = "experimental-shellfox-native")]
pub use layout_api::StampedNative0Recovery;
pub use layout_api::{
    ControllerGeometryStamp, ControllerRevocation, LayoutStamp, RecoveryCut, RecoveryId,
    StampedRecovery,
};
pub use metadata::RuntimeMetadata;
#[cfg(feature = "experimental-shellfox-native")]
pub use recovery::Native0RuntimeConfig;
mod observer_commands;
mod observers;
mod owner;
mod owner_events;
mod presentation;
mod presentation_api;
mod presentation_commands;
pub use presentation_api::*;
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
    #[cfg(feature = "experimental-shellfox-native")]
    pub fn start_native0_program_with_limits_and_hooks(
        identity: Identity,
        config: PtyConfig,
        limits: GridLimits,
        hooks: RuntimeHooks,
        native: Native0RuntimeConfig,
    ) -> Result<Self, StartError> {
        Self::start_owned_configured(
            identity, config, limits, hooks, Some(native),
            #[cfg(windows)]
            None,
        )
    }
    #[cfg(feature = "experimental-shellfox-native")]
    pub fn native0_engine_identity(
    ) -> Result<shellfox_ghostty_vt::extension::EngineIdentity, RuntimeError> {
        shellfox_ghostty_vt::extension::VerifiedNativeExtension::verify()
            .map(|verified| verified.identity())
            .map_err(|_| RuntimeError {
                code: ErrorCode::Unsupported,
                message: "Native002 local engine verification failed.".into(),
            })
    }
    #[cfg(windows)]
    pub fn start_administrator(
        identity: Identity,
        config: PtyConfig,
        helper: shellfox_terminal::elevated::InteractiveHelper,
        hooks: RuntimeHooks,
    ) -> Result<Self, StartError> {
        Self::start_owned_configured(
            identity, config, GridLimits::default(), hooks,
            #[cfg(feature = "experimental-shellfox-native")]
            None,
            Some(helper),
        )
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
            #[cfg(feature = "experimental-shellfox-native")]
            None,
            #[cfg(windows)]
            None,
        )
    }
    fn start_owned_configured(
        identity: Identity,
        config: PtyConfig,
        limits: GridLimits,
        hooks: RuntimeHooks,
        #[cfg(feature = "experimental-shellfox-native")] native: Option<Native0RuntimeConfig>,
        #[cfg(windows)] administrator: Option<shellfox_terminal::elevated::InteractiveHelper>,
    ) -> Result<Self, StartError> {
        limits
            .validate_size(config.cols, config.rows)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let (commands, incoming) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (outgoing, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let shared = Arc::new(Shared {
            screen: Mutex::new(None),
            legacy_requested: AtomicBool::new(false),
            live_output_status: AtomicU8::new(hooks.status() as u8),
            metadata: Mutex::new(None),
            phase: Mutex::new(Phase::Starting),
            abandoned: AtomicBool::new(false),
        });
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
                        #[cfg(feature = "experimental-shellfox-native")]
                        native,
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
            })
    }
    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        self.events.try_recv()
    }
    /// At most one owned UTF-8 cell frame waits here. A slow UI replaces frames,
    /// never queues them. Request Screen to publish an unchanged screen again.
    pub fn take_screen(&self) -> Option<ScreenFrame> {
        self.shared.legacy_requested.store(true, Ordering::Release);
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
        self.shared.abandoned.store(true, Ordering::Release);
    }
}
