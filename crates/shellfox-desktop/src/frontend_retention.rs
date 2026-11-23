//! Keeps the frontend's directory scopes open until the caller has reported its outcome.
use shellfox_files::FileService;
use std::{fmt, sync::Arc};

pub(crate) struct PreservedFrontendOwner {
    _scopes: Vec<Arc<FileService>>,
}
impl PreservedFrontendOwner {
    pub fn new(scopes: Vec<Arc<FileService>>) -> Self {
        Self { _scopes: scopes }
    }
    pub fn with_error(self, source: anyhow::Error) -> PreservedFrontendFailure {
        PreservedFrontendFailure {
            source,
            _retained: self,
        }
    }
}
pub(crate) struct PreservedFrontendFailure {
    source: anyhow::Error,
    _retained: PreservedFrontendOwner,
}
impl fmt::Debug for PreservedFrontendFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreservedFrontendFailure")
            .field("source", &format_args!("{}", self.source))
            .finish_non_exhaustive()
    }
}
impl fmt::Display for PreservedFrontendFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.source)
    }
}
impl std::error::Error for PreservedFrontendFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}
