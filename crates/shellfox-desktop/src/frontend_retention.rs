//! Explicit disposition for the original frontend issuer; never cleanup-on-Drop.
use shellfox_files::{
    DirectoryPreparationFailure, DirectoryPreparationOwner, DirectoryRetention, FileService,
};
use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) struct FrontendRetention {
    owner: DirectoryPreparationOwner,
    // Inline capacity exists before any create. The first failure stops preparation.
    failure: Option<DirectoryPreparationFailure>,
}
impl FrontendRetention {
    pub fn new(issuer: &Arc<FileService>) -> Self {
        Self {
            owner: issuer.directory_preparation_owner(),
            failure: None,
        }
    }
    pub fn capture_directory_failure(
        &mut self,
        failure: DirectoryPreparationFailure,
    ) -> anyhow::Error {
        self.failure = Some(failure);
        anyhow::anyhow!(
            "Native directory preparation failed; custody remains with the original frontend issuer"
        )
    }
    pub fn is_failed(&self) -> bool {
        self.failure.is_some()
    }
    pub fn preserve(mut self) -> PreservedFrontendOwner {
        // Poison recovery belongs only to this producer keeper. It does not clear
        // poison or grant ordinary-operation/authority recovery.
        let verified = self.owner.verify_retained();
        let mut last_diagnostic = Instant::now();
        loop {
            match self.owner.preserve_stale() {
                Ok(_) => break,
                Err(_) => {
                    if last_diagnostic.elapsed() >= Duration::from_secs(5) {
                        eprintln!(
                            "Frontend stale preservation is unconfirmed; original issuer remains retained."
                        );
                        last_diagnostic = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
        let summary = loop {
            match self.owner.retention() {
                Ok(summary) => break summary,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        PreservedFrontendOwner {
            _retained: self,
            summary,
            verified,
        }
    }
}
/// Explicit stale disposition, still retaining original parent/evidence. Not cleanup proof.
pub(crate) struct PreservedFrontendOwner {
    _retained: FrontendRetention,
    summary: DirectoryRetention,
    verified: shellfox_files::Result<()>,
}
impl PreservedFrontendOwner {
    pub fn with_error(self, source: anyhow::Error) -> PreservedFrontendFailure {
        PreservedFrontendFailure {
            source,
            retained: self,
        }
    }
}
/// Returnable only AFTER explicit preservation. No DirectoryPreparationFailure conversion.
pub(crate) struct PreservedFrontendFailure {
    source: anyhow::Error,
    retained: PreservedFrontendOwner,
}
impl fmt::Debug for PreservedFrontendFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreservedFrontendFailure")
            .field("source", &format_args!("{}", self.source))
            .field("retention", &self.retained.summary)
            .field("retained_verified", &self.retained.verified.is_ok())
            .finish_non_exhaustive()
    }
}
impl fmt::Display for PreservedFrontendFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; frontend artifacts explicitly preserved stale (not a cleanup/NoCreate proof)",
            self.source
        )?;
        if let Some(failure) = &self.retained._retained.failure {
            write!(f, ": {}", failure.error())?;
        }
        Ok(())
    }
}
impl std::error::Error for PreservedFrontendFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panic_without_typed_failure_keeps_the_original_issuer_until_disposition() {
        let issuer = Arc::new(FileService::local_desktop(
            shellfox_files::Access::ReadWrite,
        ));
        let original = Arc::downgrade(&issuer);
        let retention = FrontendRetention::new(&issuer);
        drop(issuer);
        let outcome = std::panic::catch_unwind(|| panic!("injected before native call"));
        assert!(outcome.is_err());
        assert!(original.upgrade().is_some());
        let preserved = retention.preserve();
        assert!(preserved.verified.is_ok());
        assert_eq!(preserved.summary.uncertain, 0);
        assert_eq!(preserved.summary.live_custody, 0);
        assert!(original.upgrade().is_some());
        drop(preserved);
        assert!(original.upgrade().is_none());
    }
}
