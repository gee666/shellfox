//! One private directory leaf inside the SAME native capability and bounded creation ledger.
use super::*;
use crate::{
    directory_plan::create::{create_private_child_retained, DirectoryAdmissionGuard},
    open_existing_private_at,
    private_creation::native_receipt,
    private_verify, DirectoryPreparationFailure, HandleIdentity, PreparedPrivateDirectoryRef,
    PrivateAccess, PrivateKind, VerifiedPrivate,
};
fn binding(value: &VerifiedPrivate, parent: &HandleIdentity) -> Result<()> {
    let receipt = native_receipt::view(PreparedPrivateDirectoryRef::Verified(value))?;
    if receipt.parent_id != parent || !parent.matches(receipt.parent)? {
        return Err(FileError::denied());
    }
    Ok(())
}
impl FileService {
    /// One validated leaf, never a path. Keep the original preparation owner outside unwind.
    /// Every failure retains this capability/ledger, including possible created artifacts.
    pub fn open_or_create_private_child(
        self: &Arc<Self>,
        leaf: &OsStr,
    ) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
        let mut admission = self.acquire_directory_admission()?;
        let outcome = self.private_child_admitted(leaf, &mut admission);
        admission.finish(outcome)
    }
    fn private_child_admitted(
        self: &Arc<Self>,
        leaf: &OsStr,
        admission: &mut DirectoryAdmissionGuard,
    ) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
        let retain = |error| DirectoryPreparationFailure::new(error, self.clone());
        admission.check_issuer(self).map_err(&retain)?;
        if !self.is_anchored_relative() {
            return Err(retain(unsupported()));
        }
        self.can_derive_root().map_err(&retain)?;
        self.can_derive_root().map_err(&retain)?;
        let parent = self.retained_anchored_directory().map_err(&retain)?;
        let original_parent = parent
            .try_clone()
            .map_err(FileError::from)
            .map_err(&retain)?
            .into_std_file();
        let parent_id =
            private_verify::object(&original_parent, PrivateKind::Directory).map_err(&retain)?;
        private_verify::parent(&original_parent).map_err(&retain)?;
        let readonly = match open_existing_private_at(
            &original_parent,
            leaf,
            PrivateKind::Directory,
            PrivateAccess::ReadOnly,
        ) {
            Ok(value) => Some(value),
            Err(error) if error.code == ErrorCode::NotFound => None,
            Err(error) => return Err(retain(error)),
        };
        if let Some(readonly) = readonly {
            binding(&readonly, &parent_id).map_err(&retain)?;
            self.authorized(true).map_err(&retain)?;
            // Explicit scoped RW reopen, not RO relabelling or inference from directory FD flags.
            let rw = native_receipt::reopen_rw(&readonly).map_err(&retain)?;
            binding(&rw, &parent_id).map_err(&retain)?;
            if rw.requested_access() != PrivateAccess::ReadWrite {
                return Err(retain(FileError::denied()));
            }
            #[cfg(windows)]
            if !rw.directory_pin_is_held() {
                return Err(retain(FileError::denied()));
            }
            let child = Dir::from_std_file(
                rw.as_file()
                    .try_clone()
                    .map_err(FileError::from)
                    .map_err(&retain)?,
            );
            // The admission owns MUTATIONS. Queries/reopens do not claim filesystem creation.
            return self.finish_private_child(leaf, &original_parent, &parent_id, child);
        }
        // Only the missing-child branch may create. Structured possible effects survive errors.
        admission.mark_effect_possible();
        let child = create_private_child_retained(self, &parent, leaf, admission.mutation())?;
        self.finish_private_child(leaf, &original_parent, &parent_id, child)
    }
    fn finish_private_child(
        self: &Arc<Self>,
        leaf: &OsStr,
        parent: &std::fs::File,
        parent_id: &HandleIdentity,
        child: Dir,
    ) -> std::result::Result<Arc<FileService>, DirectoryPreparationFailure> {
        let result = (|| {
            self.can_derive_root()?;
            let identity = private_verify::object(
                &child.try_clone()?.into_std_file(),
                PrivateKind::Directory,
            )?;
            if !parent_id.matches(parent)? {
                return Err(FileError::conflict());
            }
            let named = open_existing_private_at(
                parent,
                leaf,
                PrivateKind::Directory,
                PrivateAccess::ReadOnly,
            )?;
            binding(&named, parent_id)?;
            if named.identity() != &identity {
                return Err(FileError::conflict());
            }
            // Parent and child privacy are verify-only, including external permission changes.
            if private_verify::object(parent, PrivateKind::Directory)? != *parent_id {
                return Err(FileError::conflict());
            }
            self.authorized(true)?;
            // Reuse an existing fence Arc; private derivation preserves ALL restrictions/ledger.
            let fence = self
                .native_commit_fences
                .first()
                .ok_or_else(FileError::denied)?
                .clone();
            let derived =
                self.derive_anchored_relative(child, "native-private-child".into(), fence)?;
            self.authorized(true)?;
            Ok(Arc::new(derived))
        })();
        result.map_err(|error| DirectoryPreparationFailure::new(error, self.clone()))
    }
}
