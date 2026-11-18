//! Windows-only private DirectoryPlan entry; native custody is committed before postvalidation.
use super::*;
use crate::{
    directory_plan::create::{DirectoryAdmissionGuard, DirectoryCreatorOrigin},
    directory_walk::DirectoryWalk,
};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub(in crate::directory_plan) fn prepare_private_root(
    plan: &DirectoryPlan,
) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
    prepare_internal(plan, |_| Ok(()))
}
#[cfg(test)]
pub(super) fn prepare_with(
    plan: &DirectoryPlan,
    after_registered: impl FnMut(usize) -> Result<()>,
) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
    prepare_internal(plan, after_registered)
}
fn prepare_internal(
    plan: &DirectoryPlan,
    mut after_registered: impl FnMut(usize) -> Result<()>,
) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
    // Retention is INFALLIBLE and remains outside capture/worker unwind. Not admission.
    let keeper = plan.authority.directory_preparation_owner();
    let mut admission = plan.authority.acquire_directory_admission()?;
    let unwind_error = FileError::new(
        crate::ErrorCode::StorageFailed,
        "Private Windows preparation interrupted; original keeper retained.",
    );
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        work(plan, &mut admission, &mut after_registered)
    }));
    match outcome {
        Ok(result) => admission.finish(result),
        Err(_payload) => {
            // Unfinished admission Drop faults WITHOUT poisoning/reacquiring the global mutex.
            drop(admission);
            drop(keeper);
            Err(DirectoryPreparationFailure::new(
                unwind_error,
                plan.authority.clone(),
            ))
        }
    }
}
fn work(
    plan: &DirectoryPlan,
    admission: &mut DirectoryAdmissionGuard,
    after_registered: &mut impl FnMut(usize) -> Result<()>,
) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
    let retain = |error| DirectoryPreparationFailure::new(error, plan.authority.clone());
    admission.check_issuer(&plan.authority).map_err(&retain)?;
    plan.authority.can_derive_root().map_err(&retain)?;
    let initial = plan.current().map_err(&retain)?;
    plan.authority
        .plan_creations
        .lock()
        .map_err(|_| retain(FileError::conflict()))?
        .reserve(initial.missing.len())
        .map_err(&retain)?;
    if plan.original.missing.is_empty() {
        // Existing original provenance, explicit scoped RW reopen, NO creation/effect mark.
        let existing = PinnedPrivateDirectory::open_existing(plan, admission.mutation())?;
        return existing.handoff(plan).map_err(retain);
    }
    let mut steps = 0;
    loop {
        admission.check_issuer(&plan.authority).map_err(&retain)?;
        plan.authority.authorized(true).map_err(&retain)?;
        let current = plan.current().map_err(&retain)?;
        let ancestors = original_current_chain(plan, &current).map_err(&retain)?;
        if current.missing.is_empty() {
            let parent_index = ancestors
                .len()
                .checked_sub(2)
                .ok_or_else(|| retain(FileError::denied()))?;
            let leaf = current
                .names
                .last()
                .ok_or_else(|| retain(FileError::denied()))?
                .clone();
            let pins =
                current_pins(plan, &current, ancestors, parent_index, leaf).map_err(&retain)?;
            let directory = pins
                .ancestors
                .last()
                .ok_or_else(|| retain(FileError::denied()))?
                .try_clone()
                .map_err(FileError::from)
                .map_err(&retain)?;
            let identity = HandleIdentity::from_directory(&directory).map_err(&retain)?;
            return PinnedPrivateDirectory {
                pins,
                directory,
                identity,
            }
            .handoff(plan)
            .map_err(retain);
        }
        let parent_index = ancestors
            .len()
            .checked_sub(1)
            .ok_or_else(|| retain(FileError::denied()))?;
        let pins = current_pins(
            plan,
            &current,
            ancestors,
            parent_index,
            current.missing[0].clone(),
        )
        .map_err(&retain)?;
        plan.authority.authorized(true).map_err(&retain)?;
        // Immediately before the ACTUAL potentially-effecting native create. Queries did not mark it.
        admission.mark_effect_possible();
        let index = capture::capture_step(&pins, admission.mutation()).map_err(&retain)?;
        steps += 1;
        // Actual FD/fullID+creator provenance ALREADY in original ledger, before callbacks/clones.
        after_registered(steps).map_err(&retain)?;
        let directory = plan
            .authority
            .plan_creations
            .lock()
            .map_err(|_| retain(FileError::conflict()))?
            .clone_created(index)
            .map_err(&retain)?;
        let identity = HandleIdentity::from_directory(&directory).map_err(&retain)?;
        PinnedPrivateDirectory {
            pins,
            directory,
            identity,
        }
        .verify()
        .map_err(&retain)?;
        plan.revalidate().map_err(&retain)?;
    }
}
fn original_current_chain(plan: &DirectoryPlan, current: &DirectoryWalk) -> Result<Vec<Dir>> {
    let mut dirs = Vec::new();
    dirs.try_reserve(current.dirs.len()).map_err(|_| {
        FileError::new(
            crate::ErrorCode::Unsupported,
            "Private directory pin capacity unavailable.",
        )
    })?;
    for dir in &plan.original.dirs {
        dirs.push(dir.try_clone()?);
    }
    for index in plan.original.dirs.len()..current.dirs.len() {
        let parent = &current.identities[index - 1];
        let name = &current.names[index - 1];
        let child = &current.identities[index];
        let ledger = plan
            .authority
            .plan_creations
            .lock()
            .map_err(|_| FileError::conflict())?;
        // A foreign or Ordinary row is not private creator provenance. No permission inference.
        if ledger.created_origin(parent, name, child)
            != Some(DirectoryCreatorOrigin::PrivateCreator)
        {
            return Err(FileError::conflict());
        }
        let held = ledger
            .clone_created_matching(parent, name, child)?
            .ok_or_else(FileError::conflict)?;
        drop(ledger);
        if crate::private_verify::object(
            &held.try_clone()?.into_std_file(),
            PrivateKind::Directory,
        )? != *child
        {
            return Err(FileError::conflict());
        }
        dirs.push(held);
    }
    Ok(dirs)
}
fn current_pins(
    plan: &DirectoryPlan,
    current: &DirectoryWalk,
    ancestors: Vec<Dir>,
    parent_index: usize,
    leaf: OsString,
) -> Result<PlanPins> {
    let pins = PlanPins {
        authority: plan.authority.clone(),
        ancestors,
        identities: current.identities.clone(),
        parent_index,
        parent_identity: current
            .identities
            .get(parent_index)
            .ok_or_else(FileError::denied)?
            .clone(),
        leaf,
        configured: plan.configured.clone(),
    };
    pins.check()?;
    Ok(pins)
}
