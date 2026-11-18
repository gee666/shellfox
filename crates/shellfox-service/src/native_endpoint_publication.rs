//! One fixed endpoint slot owns publication before write and through artifact cleanup.
use crate::{
    local::NativeControlChild,
    native_endpoint_report::{
        EndpointArtifactDisposition, EndpointEvidence, EndpointPublicationOutcome,
    },
    NativeEndpointReport,
};
use anyhow::{ensure, Context, Result};
use shellfox_files::{
    ErrorCode, PrivateCleanupDisposition, PrivatePublicationPhase, PrivatePublishOutcome,
};
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
        ensure!(owner.is_alive(), "Native owner lost before endpoint read");
        let files = child.files();
        let expected = match files.read_private_leaf(OsStr::new(ENDPOINT), 4096) {
            Ok(read) => Some(read.version),
            Err(error) if error.code == ErrorCode::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        ensure!(owner.is_alive(), "Native owner lost after endpoint read");
        let mut slot = self
            .evidence
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ensure!(!slot.attempted, "Endpoint publication cannot be replayed");
        slot.attempted = true; // fixed retention slot reserved BEFORE native create
        slot.outcome = EndpointPublicationOutcome::Preparing;
        let stage = format!("endpoint-{}.tmp", uuid::Uuid::new_v4());
        match files.prepare_private_leaf_publication(
            OsStr::new(ENDPOINT),
            OsStr::new(&stage),
            expected.as_ref(),
        ) {
            Ok(attempt) => slot.attempt = Some(attempt),
            Err(failure) => {
                let (error, attempt) = failure.into_parts();
                slot.attempt = attempt; // keep original custody BEFORE diagnostic conversion
                slot.phase = Some(PrivatePublicationPhase::PreparationFailed);
                slot.outcome = EndpointPublicationOutcome::PreparationFailed;
                return Err(error.into());
            }
        }
        let (written, phase) = {
            let attempt = slot
                .attempt
                .as_mut()
                .context("Missing endpoint preparation custody")?;
            let written = attempt.write_all(bytes);
            (written, attempt.phase())
        };
        slot.phase = Some(phase);
        if let Err(error) = written {
            slot.outcome = EndpointPublicationOutcome::WriteFailed;
            return Err(error.into());
        }
        ensure!(
            owner.is_alive(),
            "Native owner lost before endpoint publication"
        );
        let (outcome, phase) = {
            let attempt = slot
                .attempt
                .as_mut()
                .context("Missing endpoint publication custody")?;
            (attempt.publish(), attempt.phase())
        };
        slot.phase = Some(phase);
        let durability = match outcome {
            PrivatePublishOutcome::Published(value) => {
                slot.outcome = EndpointPublicationOutcome::Published;
                value
            }
            PrivatePublishOutcome::NotPublished(error) => {
                slot.outcome = EndpointPublicationOutcome::NotPublished;
                return Err(error.into());
            }
            PrivatePublishOutcome::Uncertain(error) => {
                slot.outcome = EndpointPublicationOutcome::Uncertain;
                return Err(error.into());
            }
            PrivatePublishOutcome::Refused(error) => {
                slot.outcome = EndpointPublicationOutcome::Refused;
                return Err(error.into());
            }
        };
        slot.durability = Some(durability);
        let version = slot
            .attempt
            .as_ref()
            .context("Missing endpoint attempt")?
            .published_receipt()
            .context("Known visibility lacks original published receipt")?
            .version()
            .clone();
        ensure!(
            owner.is_alive(),
            "Native owner lost before endpoint read-back"
        );
        let read = files.read_private_leaf(OsStr::new(ENDPOINT), 4096)?;
        ensure!(
            read.bytes == bytes && read.version == version,
            "Endpoint read-back differs from owned publication"
        );
        ensure!(
            owner.is_alive(),
            "Native owner lost after endpoint read-back"
        );
        Ok(())
    }
    #[cfg(unix)]
    pub fn preserve_socket(&mut self, address: String) {
        self.evidence
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .socket = Some(address);
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr().lock(), "Warning: closed Unix endpoint socket artifact preserved; restart/stale accumulation acceptance remains gated.");
    }
    pub fn cleanup(&mut self) {
        let mut slot = self
            .evidence
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if slot.artifact != EndpointArtifactDisposition::NotCleaned {
            return;
        }
        if let Some(attempt) = slot.attempt.as_mut() {
            let outcome = attempt.cleanup();
            // Preserve closes our child handle, NEVER its name. Keep the actual
            // SDK attempt/evidence even on errors and after a delete request.
            attempt.preserve_stale();
            match outcome {
                Ok(PrivateCleanupDisposition::DeletePending) => {
                    slot.artifact = EndpointArtifactDisposition::DeletePending
                }
                Ok(PrivateCleanupDisposition::PreservedArtifactEvidence { .. }) => {
                    slot.artifact = EndpointArtifactDisposition::Preserved
                }
                Err(error) => {
                    slot.cleanup_error = Some(error);
                    slot.artifact = EndpointArtifactDisposition::Preserved;
                }
            }
        } else {
            slot.artifact = if slot.attempted {
                EndpointArtifactDisposition::CustodyUnknown
            } else {
                EndpointArtifactDisposition::NoOwnedAttempt
            };
        }
        if slot.artifact == EndpointArtifactDisposition::Preserved {
            use std::io::Write as _;
            let _ = writeln!(std::io::stderr().lock(), "Warning: endpoint artifact evidence preserved separately from Service/Go resource cleanup.");
        }
    }
}
