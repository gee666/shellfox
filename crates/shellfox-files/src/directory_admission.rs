//! One fixed preparation domain per original ledger. Admission never grants authority.
use super::ledger::{CreationLedger, DirectoryPreparationFailure, DirectoryPreparationOwner};
use crate::{FileError, FileService, Result};
use std::sync::{Arc, MutexGuard};
#[cfg(test)]
#[path = "directory_admission_tests.rs"]
mod tests;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DirectoryPhase {
    #[default]
    Idle,
    Active,
    Faulted,
    Preserved,
}
#[derive(Default)]
pub(super) struct Admission {
    pub phase: DirectoryPhase,
    pub serial: u64,
    pub baseline: usize,
}
/// Opaque, !Clone; claims before MUTATIONS wait and owns its real guard+same keeper.
pub(crate) struct DirectoryAdmissionGuard {
    owner: DirectoryPreparationOwner,
    serial: u64,
    effect_possible: bool,
    completed: bool,
    mutation: MutexGuard<'static, ()>,
}
impl FileService {
    pub(crate) fn acquire_directory_admission(
        self: &Arc<Self>,
    ) -> std::result::Result<DirectoryAdmissionGuard, DirectoryPreparationFailure> {
        let owner = self.directory_preparation_owner();
        let serial = (|| -> Result<u64> {
            let mut ledger = match self.plan_creations.lock() {
                Ok(ledger) => ledger,
                Err(poisoned) => {
                    let mut ledger = poisoned.into_inner();
                    // Conservative metadata only; never clear poison or rearm.
                    if ledger.admission.phase == DirectoryPhase::Idle {
                        ledger.admission.phase = DirectoryPhase::Faulted;
                    }
                    return Err(FileError::conflict());
                }
            };
            ledger.claim_admission()
        })()
        .map_err(|error| DirectoryPreparationFailure::new(error, self.clone()))?;
        // RELEASE ledger BEFORE waiting for global MUTATIONS. Never invert capture/finish order.
        let mutation = match Self::mutation_lock() {
            Ok(guard) => guard,
            Err(error) => {
                self.plan_creations
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .fault_admission(serial);
                return Err(DirectoryPreparationFailure::new(error, self.clone()));
            }
        };
        Ok(DirectoryAdmissionGuard {
            owner,
            serial,
            effect_possible: false,
            completed: false,
            mutation,
        })
    }
}
impl DirectoryAdmissionGuard {
    /// Identity correlation only, not RW/scope/revocation/native ownership proof.
    pub(crate) fn check_issuer(&self, issuer: &Arc<FileService>) -> Result<()> {
        if !Arc::ptr_eq(&self.owner.issuer, issuer)
            || !Arc::ptr_eq(&self.owner.issuer.plan_creations, &issuer.plan_creations)
        {
            return Err(FileError::conflict());
        }
        Ok(())
    }
    pub(crate) fn mutation(&self) -> &MutexGuard<'static, ()> {
        &self.mutation
    }
    /// Infallible/nonlocking; before an actual potentially-effecting create, NOT a read/reopen.
    pub(crate) fn mark_effect_possible(&mut self) {
        self.effect_possible = true;
    }
    pub(crate) fn finish<T>(
        mut self,
        outcome: std::result::Result<T, DirectoryPreparationFailure>,
    ) -> std::result::Result<T, DirectoryPreparationFailure> {
        let status = {
            let lock = self.owner.issuer.plan_creations.lock();
            let poisoned = lock.is_err();
            let mut ledger = lock.unwrap_or_else(std::sync::PoisonError::into_inner);
            ledger.finish_admission(self.serial, outcome.is_ok(), self.effect_possible, poisoned)
        };
        self.completed = true;
        match (outcome, status) {
            (Err(error), _) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(error)) => Err(DirectoryPreparationFailure::new(
                error,
                self.owner.issuer.clone(),
            )),
        }
    }
}
impl Drop for DirectoryAdmissionGuard {
    fn drop(&mut self) {
        if !self.completed {
            // Metadata quarantine ONLY. No MUTATIONS reacquire, preserve, unlink, revoke or poison clear.
            self.owner
                .issuer
                .plan_creations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .fault_admission(self.serial);
        }
    }
}
impl CreationLedger {
    pub(super) fn claim_admission(&mut self) -> Result<u64> {
        if self.admission.phase != DirectoryPhase::Idle {
            return Err(FileError::new(
                crate::ErrorCode::Unsupported,
                "Directory preparation is active, faulted or preserved.",
            ));
        }
        if self.has_unresolved() {
            self.admission.phase = DirectoryPhase::Faulted;
            return Err(FileError::conflict());
        }
        let serial = self
            .admission
            .serial
            .checked_add(1)
            .ok_or_else(FileError::conflict)?;
        self.admission = Admission {
            phase: DirectoryPhase::Active,
            serial,
            baseline: self.created_count(),
        };
        Ok(serial)
    }
    pub(super) fn finish_admission(
        &mut self,
        serial: u64,
        success: bool,
        possible: bool,
        poisoned: bool,
    ) -> Result<()> {
        if self.admission.phase != DirectoryPhase::Active || self.admission.serial != serial {
            return Err(FileError::conflict());
        }
        let unresolved = self.has_unresolved();
        if poisoned
            || unresolved
            || !success && (possible || self.created_count() != self.admission.baseline)
        {
            self.admission.phase = DirectoryPhase::Faulted;
            return Err(FileError::conflict());
        }
        self.admission.phase = DirectoryPhase::Idle;
        Ok(())
    }
    pub(super) fn fault_admission(&mut self, serial: u64) {
        if self.admission.phase == DirectoryPhase::Active && self.admission.serial == serial {
            self.admission.phase = DirectoryPhase::Faulted;
        }
    }
    pub(super) fn seal_preservation(&mut self) -> Result<()> {
        if self.admission.phase == DirectoryPhase::Active {
            return Err(FileError::new(
                crate::ErrorCode::Unsupported,
                "Directory operation is active; preservation is busy.",
            ));
        }
        self.admission.phase = DirectoryPhase::Preserved;
        Ok(())
    }
    pub(in crate::directory_plan) fn directory_phase(&self) -> DirectoryPhase {
        self.admission.phase
    }
}
