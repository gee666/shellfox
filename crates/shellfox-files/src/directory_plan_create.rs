//! Anchored creation keeps original scope/write/revocation and exact owned IDs.
use super::*;
use crate::HandleIdentity;
use std::ffi::OsStr;
#[path = "directory_admission.rs"]
mod admission;
#[path = "directory_creation_ledger.rs"]
mod ledger;
pub(crate) use admission::{DirectoryAdmissionGuard, DirectoryPhase};
pub(crate) use ledger::CreationLedger;
pub(in crate::directory_plan) use ledger::DirectoryCreatorOrigin;
pub use ledger::{DirectoryPreparationFailure, DirectoryPreparationOwner, DirectoryRetention};
#[derive(Clone, Copy, PartialEq, Eq)]
enum CreationMode {
    Ordinary,
    Private,
}
fn finish_private_attempt(
    ledger: &mut CreationLedger,
    attempt: usize,
    result: std::result::Result<crate::CreatedPrivate, crate::PrivateCreationFailure>,
) -> Result<(Dir, HandleIdentity)> {
    let created = match result {
        Ok(created) => created,
        Err(failure) => return Err(ledger.retain_failure(attempt, failure)),
    };
    let identity = created.identity().clone();
    let index = ledger.commit_created_with_origin(
        attempt,
        identity.clone(),
        Dir::from_std_file(created.into_file()),
        DirectoryCreatorOrigin::PrivateCreator,
    );
    // The original File is already in the original owner before this can fail.
    Ok((ledger.clone_created(index)?, identity))
}
/// UNDER-LOCK contract: caller retains the SAME MUTATIONS guard across the
/// complete scoped child operation. This helper NEVER acquires that lock.
/// The explicit crate-private guard borrow is not a new authority token.
pub(crate) fn create_private_child_retained(
    authority: &Arc<FileService>,
    parent: &Dir,
    name: &OsStr,
    _mutation: &std::sync::MutexGuard<'_, ()>,
) -> std::result::Result<Dir, DirectoryPreparationFailure> {
    let result = (|| -> Result<Dir> {
        if !authority.is_anchored_relative() {
            return Err(FileError::new(
                crate::ErrorCode::Unsupported,
                "Native private child creation requires anchored-relative authority.",
            ));
        }
        let text = name.to_str().ok_or_else(FileError::denied)?;
        paths::validate(text)?;
        let mut components = Path::new(name).components();
        if text.contains(['/', '\\', ':'])
            || !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            return Err(FileError::denied());
        }
        authority.authorized(true)?;
        let actual = authority.retained_anchored_directory()?;
        let parent_id = HandleIdentity::from_directory(parent)?;
        if parent_id != HandleIdentity::from_directory(&actual)? {
            return Err(FileError::denied());
        }
        let original_parent = parent.try_clone()?.into_std_file();
        let held_parent = parent.try_clone()?;
        let leaf = name.to_owned();
        let mut ledger = authority
            .plan_creations
            .lock()
            .map_err(|_| FileError::conflict())?;
        ledger.reserve(1)?;
        authority.authorized(true)?;
        let attempt = ledger.begin(held_parent, parent_id, leaf);
        let result = finish_private_attempt(
            &mut ledger,
            attempt,
            crate::create_private_directory_at(&original_parent, name),
        )?;
        authority.authorized(true)?;
        Ok(result.0)
    })();
    result.map_err(|error| DirectoryPreparationFailure::new(error, authority.clone()))
}
fn create_child(
    ledger: &mut CreationLedger,
    parent: &Dir,
    parent_id: &HandleIdentity,
    name: &OsString,
    mode: CreationMode,
    _mutation: &std::sync::MutexGuard<'_, ()>,
) -> Result<(Dir, HandleIdentity)> {
    // All fallible handle/name preparation occurs before the registered syscall.
    let original_parent = parent.try_clone()?.into_std_file();
    let held_parent = parent.try_clone()?;
    let leaf = name.clone();
    let attempt = ledger.begin(held_parent, parent_id.clone(), leaf);
    if mode == CreationMode::Private {
        return finish_private_attempt(
            ledger,
            attempt,
            crate::create_private_directory_at(&original_parent, name),
        );
    }
    // Any ordinary mkdir/post-open failure retains the pre-registered possible
    // artifact row. It is not a NoCreate claim or permission to unlink by name.
    match parent.create_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(FileError::conflict())
        }
        Err(error) => return Err(error.into()),
    }
    let child = paths::descend(parent.try_clone()?, Path::new(name))?;
    let identity = HandleIdentity::from_directory(&child)?;
    let index = ledger.commit_created(attempt, identity.clone(), child);
    Ok((ledger.clone_created(index)?, identity))
}
fn verify_private_root(plan: &DirectoryWalk, expected: &HandleIdentity) -> Result<()> {
    let index = plan.dirs.len().checked_sub(2).ok_or_else(|| {
        FileError::new(
            crate::ErrorCode::Unsupported,
            "A private root requires a retained parent and leaf.",
        )
    })?;
    let parent = plan.dirs[index].try_clone()?.into_std_file();
    let leaf = plan.names.last().ok_or_else(FileError::denied)?;
    let verified = crate::open_existing_private_at(
        &parent,
        leaf,
        crate::PrivateKind::Directory,
        crate::PrivateAccess::ReadWrite,
    )?;
    if verified.identity() != expected {
        return Err(FileError::conflict());
    }
    verified.verify_binding()
}
impl DirectoryPlan {
    /// Caller retains/revalidates its complete topology at each root boundary.
    /// This creates only through this ORIGINAL capability and never upgrades it.
    pub fn create_approved_root(
        &self,
    ) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
        self.create_root_admitted(CreationMode::Ordinary)
    }
    /// New components are private AT birth through the shared native primitive.
    /// Existing target privacy is verify-only, never repaired/certified at birth.
    /// Requires trusted native parent/namespace custody; not VFS confinement.
    pub fn create_private_approved_root(
        &self,
    ) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
        #[cfg(windows)]
        {
            super::windows_atomic_creation_anchor::prepare_private_root(self)
        }
        #[cfg(not(windows))]
        {
            self.create_root_admitted(CreationMode::Private)
        }
    }
    fn create_root_admitted(
        &self,
        mode: CreationMode,
    ) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
        let mut admission = self.authority.acquire_directory_admission()?;
        let outcome = self
            .create_root(mode, &mut admission)
            .map_err(|error| DirectoryPreparationFailure::new(error, self.authority.clone()));
        admission.finish(outcome)
    }
    fn create_root(
        &self,
        mode: CreationMode,
        admission: &mut DirectoryAdmissionGuard,
    ) -> Result<Arc<FileService>> {
        admission.check_issuer(&self.authority)?;
        self.authority.can_derive_root()?;
        let mut current = self.current()?;
        self.authority
            .plan_creations
            .lock()
            .map_err(|_| FileError::conflict())?
            .reserve(current.missing.len())?;
        let names = current.missing.clone();
        let mut parent = current.dirs.pop().ok_or_else(FileError::denied)?;
        let mut parent_id = HandleIdentity::from_directory(&parent)?;
        for name in names {
            let fresh = self.current()?;
            if fresh.missing.first() != Some(&name)
                || !fresh.identities.last().is_some_and(|id| id == &parent_id)
            {
                return Err(FileError::conflict());
            }
            self.authority.authorized(true)?;
            let (child, child_id) = {
                let mut ledger = self
                    .authority
                    .plan_creations
                    .lock()
                    .map_err(|_| FileError::conflict())?;
                admission.mark_effect_possible();
                create_child(
                    &mut ledger,
                    &parent,
                    &parent_id,
                    &name,
                    mode,
                    admission.mutation(),
                )?
            };
            parent = child;
            parent_id = child_id;
            self.current()?;
        }
        let final_plan = self.current()?;
        if !final_plan.missing.is_empty()
            || !final_plan
                .identities
                .last()
                .is_some_and(|id| id == &parent_id)
        {
            return Err(FileError::conflict());
        }
        if mode == CreationMode::Private {
            verify_private_root(&final_plan, &parent_id)?;
            self.current()?;
        }
        self.authority.authorized(true)?;
        Ok(Arc::new(
            self.authority
                .derive_root(self.configured.clone(), parent)?,
        ))
    }
}
