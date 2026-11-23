//! What happened to endpoint.json, independent of activity and process cleanup.
use shellfox_files::{private::PrivateVersion, FileService};
use std::sync::{Arc, Mutex};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointPublicationOutcome {
    NotAttempted,
    Failed,
    Published,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointArtifactDisposition {
    NotCleaned,
    /// Nothing was published, so there is nothing to remove.
    NoOwnedAttempt,
    Deleted,
    /// The file changed since we wrote it, or removal failed; it was left in place.
    Preserved,
}
#[derive(Clone)]
pub struct NativeEndpointReport {
    outcome: EndpointPublicationOutcome,
    artifact: EndpointArtifactDisposition,
    preserved_sockets: usize,
    cleanup_error: Option<shellfox_files::ErrorCode>,
}
pub(crate) struct EndpointEvidence {
    pub published: Option<(Arc<FileService>, PrivateVersion)>,
    pub outcome: EndpointPublicationOutcome,
    pub artifact: EndpointArtifactDisposition,
    pub socket: Option<String>,
    pub cleanup_error: Option<shellfox_files::ErrorCode>,
    pub attempted: bool,
}
impl Default for EndpointEvidence {
    fn default() -> Self {
        Self {
            published: None,
            outcome: EndpointPublicationOutcome::NotAttempted,
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
            outcome: state.outcome,
            artifact: state.artifact,
            preserved_sockets: usize::from(state.socket.is_some()),
            cleanup_error: state.cleanup_error,
        })
    }
    pub fn publication_outcome(&self) -> EndpointPublicationOutcome {
        self.outcome
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
            .field("outcome", &self.outcome)
            .field("artifact", &self.artifact)
            .field("preserved_sockets", &self.preserved_sockets)
            .field("cleanup_error", &self.cleanup_error)
            .finish()
    }
}
