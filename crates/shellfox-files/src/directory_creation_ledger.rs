//! One original-issuer creation/retention book; no implicit cleanup or unlink.
use crate::{
    CreationCustody, FileError, FileService, HandleIdentity, PrivateCreationFailure,
    PrivatePreservationState, Result,
};
use cap_std::fs::Dir;
use std::{
    ffi::{OsStr, OsString},
    sync::Arc,
};
const MAX_CREATED: usize = 2048;
#[cfg(test)]
#[path = "directory_preparation_owner_tests.rs"]
mod owner_tests;
#[cfg(test)]
#[path = "directory_preservation_tests.rs"]
mod preservation_tests;
struct Created {
    parent: HandleIdentity,
    name: OsString,
    child: HandleIdentity,
    directory: Option<Dir>,
    held_parent: Dir,
    origin: DirectoryCreatorOrigin,
}
enum Attempt {
    Entered {
        parent: Dir,
        identity: HandleIdentity,
        leaf: OsString,
        preserved: bool,
    },
    Retained(CreationCustody),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::directory_plan) enum DirectoryCreatorOrigin {
    Ordinary,
    PrivateCreator,
}
#[derive(Default)]
pub(crate) struct CreationLedger {
    entries: Vec<Created>,
    attempts: Vec<Option<Attempt>>,
    pub(super) admission: super::admission::Admission,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryRetention {
    pub created: usize,
    pub uncertain: usize,
    pub live_custody: usize,
    pub stale_evidence: usize,
}
#[path = "directory_retention_owner.rs"]
mod owner;
pub use owner::{DirectoryPreparationFailure, DirectoryPreparationOwner};
impl CreationLedger {
    pub(in crate::directory_plan) fn proves(
        &self,
        parent: &HandleIdentity,
        name: &OsString,
        child: &HandleIdentity,
    ) -> bool {
        self.created_origin(parent, name, child).is_some()
    }
    pub(in crate::directory_plan) fn created_origin(
        &self,
        parent: &HandleIdentity,
        name: &OsString,
        child: &HandleIdentity,
    ) -> Option<DirectoryCreatorOrigin> {
        self.entries
            .iter()
            .find(|entry| {
                entry.directory.is_some()
                    && entry.name == *name
                    && entry.parent == *parent
                    && entry.child == *child
            })
            .map(|entry| entry.origin)
    }
    pub(super) fn created_count(&self) -> usize {
        self.entries.len()
    }
    pub(super) fn has_unresolved(&self) -> bool {
        self.attempts.iter().any(Option::is_some)
    }
    pub(in crate::directory_plan) fn reserve(&mut self, count: usize) -> Result<()> {
        if matches!(
            self.directory_phase(),
            super::DirectoryPhase::Faulted | super::DirectoryPhase::Preserved
        ) {
            return Err(FileError::new(
                crate::ErrorCode::Unsupported,
                "Directory preparation admission is quarantined or preserved.",
            ));
        }
        let used =
            self.entries.len() + self.attempts.iter().filter(|entry| entry.is_some()).count();
        if used
            .checked_add(count)
            .is_none_or(|total| total > MAX_CREATED)
        {
            return Err(FileError::new(
                crate::ErrorCode::Unsupported,
                "Directory creation retention capacity is full.",
            ));
        }
        // Allocate all successful and failed-result capacity BEFORE native create.
        self.entries.try_reserve(count).map_err(|_| {
            FileError::new(
                crate::ErrorCode::Unsupported,
                "Directory creation retention memory is unavailable.",
            )
        })?;
        self.attempts.try_reserve(count).map_err(|_| {
            FileError::new(
                crate::ErrorCode::Unsupported,
                "Directory creation retention memory is unavailable.",
            )
        })?;
        Ok(())
    }
    pub(in crate::directory_plan) fn begin(
        &mut self,
        parent: Dir,
        identity: HandleIdentity,
        leaf: OsString,
    ) -> usize {
        let value = Some(Attempt::Entered {
            parent,
            identity,
            leaf,
            preserved: false,
        });
        if let Some(index) = self.attempts.iter().position(Option::is_none) {
            self.attempts[index] = value;
            index
        } else {
            let index = self.attempts.len();
            self.attempts.push(value);
            index
        }
    }
    pub(in crate::directory_plan) fn retain_failure(
        &mut self,
        index: usize,
        failure: PrivateCreationFailure,
    ) -> FileError {
        let (error, custody) = failure.into_parts();
        self.attempts[index] = custody.map(Attempt::Retained);
        error
    }
    pub(in crate::directory_plan) fn commit_created(
        &mut self,
        index: usize,
        child: HandleIdentity,
        directory: Dir,
    ) -> usize {
        self.commit_created_with_origin(index, child, directory, DirectoryCreatorOrigin::Ordinary)
    }
    pub(in crate::directory_plan) fn commit_created_with_origin(
        &mut self,
        index: usize,
        child: HandleIdentity,
        directory: Dir,
        origin: DirectoryCreatorOrigin,
    ) -> usize {
        let Some(Attempt::Entered {
            parent,
            identity,
            leaf,
            ..
        }) = self.attempts[index].take()
        else {
            unreachable!("Original reserved creation row")
        };
        let created = self.entries.len();
        self.entries.push(Created {
            parent: identity,
            name: leaf,
            child,
            directory: Some(directory),
            held_parent: parent,
            origin,
        });
        created
    }
    /// Live exact original row only: no index guessing, replan or permission proof.
    pub(in crate::directory_plan) fn clone_created_matching(
        &self,
        parent: &HandleIdentity,
        leaf: &OsStr,
        child: &HandleIdentity,
    ) -> Result<Option<Dir>> {
        let Some(entry) = self.entries.iter().find(|entry| {
            entry.directory.is_some()
                && entry.parent == *parent
                && entry.name == leaf
                && entry.child == *child
        }) else {
            return Ok(None);
        };
        let directory = entry.directory.as_ref().ok_or_else(FileError::conflict)?;
        if HandleIdentity::from_directory(&entry.held_parent)? != *parent
            || HandleIdentity::from_directory(directory)? != *child
        {
            return Err(FileError::conflict());
        }
        Ok(Some(directory.try_clone()?))
    }
    pub(in crate::directory_plan) fn clone_created(&self, index: usize) -> Result<Dir> {
        let entry = self.entries.get(index).ok_or_else(FileError::conflict)?;
        self.clone_created_matching(&entry.parent, &entry.name, &entry.child)?
            .ok_or_else(FileError::conflict)
    }
    fn summary(&self) -> DirectoryRetention {
        let mut result = DirectoryRetention {
            created: self
                .entries
                .iter()
                .filter(|entry| entry.directory.is_some())
                .count(),
            uncertain: 0,
            live_custody: 0,
            stale_evidence: self
                .entries
                .iter()
                .filter(|entry| entry.directory.is_none())
                .count(),
        };
        for attempt in self.attempts.iter().flatten() {
            match attempt {
                Attempt::Entered { preserved, .. } => {
                    if *preserved {
                        result.stale_evidence += 1
                    } else {
                        result.uncertain += 1
                    }
                }
                Attempt::Retained(custody) => match custody.preservation_state() {
                    PrivatePreservationState::LiveCustody => result.live_custody += 1,
                    PrivatePreservationState::StaleArtifactEvidence => result.stale_evidence += 1,
                },
            }
        }
        result
    }
    fn verify_retained(&self) -> Result<()> {
        for entry in &self.entries {
            if let Some(directory) = &entry.directory {
                if entry.child != HandleIdentity::from_directory(directory)? {
                    return Err(FileError::conflict());
                }
            }
            if entry.parent != HandleIdentity::from_directory(&entry.held_parent)? {
                return Err(FileError::conflict());
            }
        }
        for attempt in self.attempts.iter().flatten() {
            match attempt {
                Attempt::Entered {
                    parent, identity, ..
                } => {
                    if *identity != HandleIdentity::from_directory(parent)? {
                        return Err(FileError::conflict());
                    }
                }
                Attempt::Retained(custody) => custody.verify_retained_parent()?,
            }
        }
        Ok(())
    }
    fn preserve_stale(&mut self) {
        // End only this ledger's own child pins; keep original parents/full IDs.
        // Closed child evidence can no longer prove a newly adopted prefix.
        for entry in &mut self.entries {
            entry.directory = None;
        }
        for attempt in self.attempts.iter_mut().flatten() {
            match attempt {
                Attempt::Entered { preserved, .. } => *preserved = true,
                Attempt::Retained(custody) => {
                    custody.preserve_stale();
                }
            }
        }
    }
}
