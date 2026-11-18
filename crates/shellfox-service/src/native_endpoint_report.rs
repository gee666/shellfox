//! Endpoint artifacts are independent of activity, presentation and process cleanup.
use shellfox_files::{PrivateDurability, PrivatePublicationPhase, ScopedPrivatePublication};
use std::sync::{Arc, Mutex};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointPublicationOutcome {
    NotAttempted,
    Preparing,
    PreparationFailed,
    WriteFailed,
    NotPublished,
    Refused,
    Uncertain,
    Published,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointArtifactDisposition {
    NotCleaned,
    NoOwnedAttempt,
    /// Preparation was entered but original publication custody is missing.
    /// This is not evidence of artifact absence or NoPublication.
    CustodyUnknown,
    DeletePending,
    Preserved,
}
#[derive(Clone)]
pub struct NativeEndpointReport {
    phase: Option<PrivatePublicationPhase>,
    outcome: EndpointPublicationOutcome,
    durability: Option<PrivateDurability>,
    artifact: EndpointArtifactDisposition,
    preserved_sockets: usize,
    cleanup_error: Option<shellfox_files::ErrorCode>,
    _evidence: Arc<Mutex<EndpointEvidence>>,
}
pub(crate) struct EndpointEvidence {
    pub attempt: Option<ScopedPrivatePublication>,
    pub phase: Option<PrivatePublicationPhase>,
    pub outcome: EndpointPublicationOutcome,
    pub durability: Option<PrivateDurability>,
    pub artifact: EndpointArtifactDisposition,
    pub socket: Option<String>,
    pub cleanup_error: Option<shellfox_files::FileError>,
    pub attempted: bool,
}
impl Default for EndpointEvidence {
    fn default() -> Self {
        Self {
            attempt: None,
            phase: None,
            outcome: EndpointPublicationOutcome::NotAttempted,
            durability: None,
            artifact: EndpointArtifactDisposition::NotCleaned,
            socket: None,
            cleanup_error: None,
            attempted: false,
        }
    }
}
impl NativeEndpointReport {
    pub(crate) fn snapshot(evidence: &Arc<Mutex<EndpointEvidence>>) -> Arc<Self> {
        let state = evidence.lock().unwrap_or_else(|poison| poison.into_inner());
        Arc::new(Self {
            phase: state.phase,
            outcome: state.outcome,
            durability: state.durability,
            artifact: state.artifact,
            preserved_sockets: usize::from(state.socket.is_some()),
            cleanup_error: state.cleanup_error.as_ref().map(|error| error.code),
            _evidence: evidence.clone(),
        })
    }
    pub fn publication_phase(&self) -> Option<PrivatePublicationPhase> {
        self.phase
    }
    pub fn publication_outcome(&self) -> EndpointPublicationOutcome {
        self.outcome
    }
    pub fn durability(&self) -> Option<PrivateDurability> {
        self.durability
    }
    pub fn artifact_disposition(&self) -> EndpointArtifactDisposition {
        self.artifact
    }
    pub fn preserved_socket_count(&self) -> usize {
        self.preserved_sockets
    }
    pub fn cleanup_error(&self) -> bool {
        self.cleanup_error.is_some()
    }
    pub fn cleanup_error_code(&self) -> Option<shellfox_files::ErrorCode> {
        self.cleanup_error
    }
}
impl std::fmt::Debug for NativeEndpointReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeEndpointReport")
            .field("phase", &self.phase)
            .field("outcome", &self.outcome)
            .field("durability", &self.durability)
            .field("artifact", &self.artifact)
            .field("preserved_sockets", &self.preserved_sockets)
            .field("cleanup_error", &self.cleanup_error)
            .finish_non_exhaustive()
    }
}
