use super::*;
impl PlanPins {
    pub(super) fn retain(
        plan: &DirectoryPlan,
        parent_index: usize,
        leaf: OsString,
    ) -> Result<Self> {
        plan.authority.can_derive_root()?;
        plan.revalidate()?;
        let value = Self {
            authority: plan.authority.clone(),
            ancestors: plan
                .original
                .dirs
                .iter()
                .map(Dir::try_clone)
                .collect::<std::io::Result<Vec<_>>>()?,
            identities: plan.original.identities.clone(),
            parent_index,
            parent_identity: plan
                .original
                .identities
                .get(parent_index)
                .ok_or_else(FileError::denied)?
                .clone(),
            leaf,
            configured: plan.configured.clone(),
        };
        value.check()?;
        Ok(value)
    }
    pub(super) fn parent(&self) -> Result<&Dir> {
        self.ancestors
            .get(self.parent_index)
            .ok_or_else(FileError::denied)
    }
    pub(super) fn check(&self) -> Result<()> {
        self.authority.can_derive_root()?;
        if self.ancestors.len() != self.identities.len() || self.ancestors.is_empty() {
            return Err(FileError::conflict());
        }
        for (dir, id) in self.ancestors.iter().zip(&self.identities) {
            if HandleIdentity::from_directory(dir)? != *id {
                return Err(FileError::conflict());
            }
        }
        let parent = self.parent()?.try_clone()?.into_std_file();
        if !self.parent_identity.matches(&parent)? {
            return Err(FileError::conflict());
        }
        // Original-plan namespace proof, not generic File-based parent approval.
        // Unknown owner is rejected; no ACL repair or arbitrary File/bool issuer.
        crate::private_security::Security::new(true)?.require_owner(&parent)?;
        self.authority.authorized(true)
    }
}
impl PinnedPrivateDirectory {
    pub(super) fn verify(&self) -> Result<()> {
        self.pins.check()?;
        let file = self.directory.try_clone()?.into_std_file();
        if crate::private_verify::object(&file, PrivateKind::Directory)? != self.identity {
            return Err(FileError::conflict());
        }
        let parent = self.pins.parent()?.try_clone()?.into_std_file();
        let named = crate::private_native::open(
            &parent,
            &self.pins.leaf,
            PrivateKind::Directory,
            crate::PrivateAccess::ReadOnly,
        )?;
        if crate::private_verify::object(&named, PrivateKind::Directory)? != self.identity {
            return Err(FileError::conflict());
        }
        self.pins.check()
    }
    pub(super) fn open_existing(
        plan: &DirectoryPlan,
        _mutation: &std::sync::MutexGuard<'_, ()>,
    ) -> std::result::Result<Self, DirectoryPreparationFailure> {
        let result = (|| {
            plan.authority.can_derive_root()?;
            plan.revalidate()?;
            if !plan.original.missing.is_empty() {
                return Err(FileError::denied());
            }
            let parent_index = plan
                .original
                .dirs
                .len()
                .checked_sub(2)
                .ok_or_else(FileError::denied)?;
            let leaf = plan
                .original
                .names
                .last()
                .ok_or_else(FileError::denied)?
                .clone();
            let pins = PlanPins::retain(plan, parent_index, leaf)?;
            let expected = plan
                .original
                .identities
                .last()
                .ok_or_else(FileError::denied)?;
            let original = plan
                .original
                .dirs
                .last()
                .ok_or_else(FileError::denied)?
                .try_clone()?
                .into_std_file();
            if crate::private_verify::object(&original, PrivateKind::Directory)? != *expected {
                return Err(FileError::conflict());
            }
            let parent = pins.parent()?.try_clone()?.into_std_file();
            pins.authority.authorized(true)?;
            // Verify-only current existing object, SAME scoped parent/leaf, real noDELETE/share3.
            // This original-plan branch does not change generic File-based verification.
            let file = crate::private_native::open_rw_pinned_directory(&parent, &pins.leaf)?;
            if crate::private_verify::object(&file, PrivateKind::Directory)? != *expected {
                return Err(FileError::conflict());
            }
            let value = Self {
                pins,
                directory: Dir::from_std_file(file),
                identity: expected.clone(),
            };
            value.verify()?;
            plan.revalidate()?;
            Ok(value)
        })();
        result.map_err(|error| DirectoryPreparationFailure::new(error, plan.authority.clone()))
    }
    pub(super) fn handoff(&self, plan: &DirectoryPlan) -> Result<Arc<FileService>> {
        if !Arc::ptr_eq(&self.pins.authority, &plan.authority)
            || plan.configured != self.pins.configured
        {
            return Err(FileError::denied());
        }
        self.verify()?;
        plan.revalidate()?;
        let current = plan.current()?;
        if !current.missing.is_empty() || current.identities.last() != Some(&self.identity) {
            return Err(FileError::conflict());
        }
        let root = Arc::new(
            self.pins
                .authority
                .derive_root(self.pins.configured.clone(), self.directory.try_clone()?)?,
        );
        plan.revalidate()?;
        self.verify()?;
        Ok(root)
    }
    // Release ONLY this view's pins. Original ledger/derived roots/plans have independent real pins.
    // No unlink or newly granted cleanup authority. Caller explicitly preserves the ORIGINAL keeper.
    #[cfg(test)]
    pub(super) fn release(self) -> PreservedEvidence {
        PreservedEvidence {
            parent: self.pins.parent_identity,
            child: self.identity,
            leaf: self.pins.leaf,
        }
    }
}
