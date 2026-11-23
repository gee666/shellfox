//! endpoint.json: written once per run, removed at cleanup only if it is still ours.
use crate::{
    local::NativeControlChild,
    native::endpoint_report::{
        EndpointArtifactDisposition, EndpointEvidence, EndpointPublicationOutcome,
    },
    NativeEndpointReport,
};
use anyhow::{ensure, Result};
use shellfox_files::ErrorCode;
use shellfox_transport::AcquiredOwnershipView;
use std::{
    ffi::OsStr,
    sync::{Arc, Mutex},
};
const ENDPOINT: &str = "endpoint.json";
pub(super) struct EndpointPublication {
    evidence: Arc<Mutex<EndpointEvidence>>,
}
impl EndpointPublication {
    pub fn new() -> Self {
        Self {
            evidence: Arc::new(Mutex::new(EndpointEvidence::default())),
        }
    }
    pub fn report(&self) -> Arc<NativeEndpointReport> {
        NativeEndpointReport::snapshot(&self.evidence)
    }
    pub fn publish(
        &mut self,
        child: &NativeControlChild,
        owner: &AcquiredOwnershipView<'_>,
        bytes: &[u8],
    ) -> Result<()> {
        ensure!(bytes.len() <= 4096, "Endpoint record exceeds its bound");
        let mut slot = self
            .evidence
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ensure!(!slot.attempted, "Endpoint publication cannot be replayed");
        slot.attempted = true;
        slot.outcome = EndpointPublicationOutcome::Failed;
        ensure!(owner.is_alive(), "Native owner lost before endpoint write");
        let files = child.files();
        // A previous run's record is replaced only if it is unchanged since this read.
        let expected = match files.read_private(OsStr::new(ENDPOINT), 4096) {
            Ok(read) => Some(read.version),
            Err(error) if error.code == ErrorCode::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let version = files.write_private(OsStr::new(ENDPOINT), bytes, expected.as_ref())?;
        slot.outcome = EndpointPublicationOutcome::Published;
        slot.published = Some((files.clone(), version.clone()));
        let read = files.read_private(OsStr::new(ENDPOINT), 4096)?;
        ensure!(
            read.bytes == bytes && read.version == version,
            "Endpoint read-back differs from the written record"
        );
        ensure!(owner.is_alive(), "Native owner lost after endpoint write");
        Ok(())
    }
    #[cfg(unix)]
    pub fn preserve_socket(&mut self, address: String) {
        self.evidence
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .socket = Some(address);
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr().lock(),
            "Warning: the closed Unix endpoint socket file was left in place."
        );
    }
    pub fn cleanup(&mut self) {
        let mut slot = self
            .evidence
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if slot.artifact != EndpointArtifactDisposition::NotCleaned {
            return;
        }
        let Some((files, version)) = slot.published.take() else {
            slot.artifact = EndpointArtifactDisposition::NoOwnedAttempt;
            return;
        };
        slot.artifact = match files.remove_private(OsStr::new(ENDPOINT), &version) {
            Ok(()) => EndpointArtifactDisposition::Deleted,
            Err(error) if error.code == ErrorCode::NotFound => EndpointArtifactDisposition::Deleted,
            Err(error) => {
                slot.cleanup_error = Some(error.code);
                use std::io::Write as _;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "Warning: endpoint.json changed or could not be removed; it was left in place."
                );
                EndpointArtifactDisposition::Preserved
            }
        };
    }
}
