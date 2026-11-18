//! Retained evidence after a native create may have happened. Never an implicit FileError conversion.
use crate::{FileError, PrivateIdentity, PrivateKind};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateCreationEvidence {
    /// A native creation call was entered; a named artifact may exist even without a returned FD.
    CreationMayHaveOccurred,
    /// The actual native returned handle is retained, but post-create validation did not finish.
    NativeHandleRetained,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivatePreservationState {
    LiveCustody,
    StaleArtifactEvidence,
}

pub struct CreationCustody {
    pub(crate) parent: File,
    pub(crate) parent_id: PrivateIdentity,
    pub(crate) leaf: OsString,
    pub(crate) kind: PrivateKind,
    pub(crate) file: Option<File>,
    pub(crate) identity: Option<PrivateIdentity>,
    pub(crate) lease: bool,
    pub(crate) pin: bool,
    pub(crate) evidence: PrivateCreationEvidence,
    pub(crate) preservation: PrivatePreservationState,
}
impl CreationCustody {
    pub fn as_file(&self) -> Option<&File> {
        self.file.as_ref()
    }
    pub fn identity(&self) -> Option<&PrivateIdentity> {
        self.identity.as_ref()
    }
    pub fn parent_identity(&self) -> &PrivateIdentity {
        &self.parent_id
    }
    /// Identity check only; caller still retains its original logical scope/revocation fence.
    pub fn verify_retained_parent(&self) -> crate::Result<()> {
        if self.parent_id.matches(&self.parent)? {
            Ok(())
        } else {
            Err(FileError::conflict())
        }
    }
    pub fn leaf(&self) -> &OsStr {
        &self.leaf
    }
    pub fn kind(&self) -> PrivateKind {
        self.kind
    }
    pub fn evidence(&self) -> PrivateCreationEvidence {
        self.evidence
    }
    pub fn preservation_state(&self) -> PrivatePreservationState {
        self.preservation
    }
    pub fn holds_exclusive_lease(&self) -> bool {
        self.lease
    }
    pub fn directory_pin_is_held(&self) -> bool {
        self.pin
    }
    /// Explicitly close this CHILD handle while retaining original-parent/stale-artifact evidence.
    /// This is NOT unlink, a NoOwner proof, or proof that any external process/resource stopped.
    pub fn preserve_stale(&mut self) -> PrivatePreservationState {
        self.file = None;
        self.lease = false;
        self.pin = false;
        self.preservation = PrivatePreservationState::StaleArtifactEvidence;
        self.preservation
    }
}
impl std::fmt::Debug for CreationCustody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreationCustody")
            .field("kind", &self.kind)
            .field("evidence", &self.evidence)
            .field("preservation", &self.preservation)
            .field("handle_retained", &self.file.is_some())
            .finish()
    }
}
#[derive(Debug)]
pub struct PrivateCreationFailure {
    pub(crate) error: FileError,
    pub(crate) custody: Option<Box<CreationCustody>>,
}
impl PrivateCreationFailure {
    pub fn error(&self) -> &FileError {
        &self.error
    }
    pub fn custody(&self) -> Option<&CreationCustody> {
        self.custody.as_deref()
    }
    pub fn custody_mut(&mut self) -> Option<&mut CreationCustody> {
        self.custody.as_deref_mut()
    }
    pub fn into_parts(self) -> (FileError, Option<CreationCustody>) {
        (self.error, self.custody.map(|custody| *custody))
    }
}
impl std::fmt::Display for PrivateCreationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
// Deliberately NOT std::error::Error: blanket anyhow `?` must not erase retained custody.
