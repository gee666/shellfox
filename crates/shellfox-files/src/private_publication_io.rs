use super::*;
fn expected(value: &OwnedPrivatePublication) -> Result<()> {
    if let Some(expected) = &value.expected {
        let read = super::super::metadata::bounded_private_read_at(
            &value.parent,
            &value.destination,
            PRIVATE_METADATA_MAX_BYTES,
        )?;
        if &read.version != expected {
            return Err(FileError::conflict());
        }
    } else {
        let dir = cap_std::fs::Dir::from_std_file(value.parent.try_clone()?);
        match dir.symlink_metadata(&value.destination) {
            Ok(_) => return Err(FileError::conflict()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
pub(super) fn prepublish(value: &OwnedPrivatePublication) -> Result<()> {
    value.verify_retained_parent()?;
    private_verify::parent(&value.parent)?;
    let stage = value.stage.as_ref().ok_or_else(state_error)?;
    stage.verify_binding()?;
    let current = read_file(stage.as_file(), PRIVATE_METADATA_MAX_BYTES)?;
    if Some(&current.version) != value.owned_version.as_ref()
        || current.version.identity() != stage.identity()
    {
        return Err(FileError::conflict());
    }
    stage.as_file().sync_all()?;
    expected(value)?;
    stage.verify_binding()?;
    private_verify::parent(&value.parent)?;
    Ok(())
}
#[cfg(test)]
pub(super) fn rename(
    parent: &File,
    stage: &OsStr,
    destination: &OsStr,
    file: &File,
    replace: bool,
) -> Result<()> {
    rename_checked(parent, stage, destination, file, replace, &|| Ok(()))
}
pub(super) fn rename_checked(
    parent: &File,
    stage: &OsStr,
    destination: &OsStr,
    file: &File,
    replace: bool,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    #[cfg(windows)]
    {
        let _ = stage;
        crate::platform::rename_held_checked(file, parent, destination, replace, check)
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let _ = file;
        let stage = std::ffi::CString::new(stage.as_bytes()).map_err(|_| FileError::denied())?;
        let destination =
            std::ffi::CString::new(destination.as_bytes()).map_err(|_| FileError::denied())?;
        check()?;
        if replace {
            rustix::fs::renameat(parent, stage.as_c_str(), parent, destination.as_c_str())
                .map_err(std::io::Error::from)?;
        } else {
            rustix::fs::renameat_with(
                parent,
                stage.as_c_str(),
                parent,
                destination.as_c_str(),
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(std::io::Error::from)?;
        }
        Ok(())
    }
}
pub(super) fn cleanup(value: &mut OwnedPrivatePublication) -> Result<PrivateCleanupDisposition> {
    if value.phase == PrivatePublicationPhase::CleanupDeletePending {
        return Ok(PrivateCleanupDisposition::DeletePending);
    }
    value.verify_retained_parent()?;
    let live = value.stage.is_some()
        || value.published.is_some()
        || value
            .failed_creation
            .as_ref()
            .is_some_and(|v| v.as_file().is_some());
    #[cfg(unix)]
    {
        Ok(PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: live,
        })
    }
    #[cfg(windows)]
    {
        if matches!(
            value.phase,
            PrivatePublicationPhase::Uncertain
                | PrivatePublicationPhase::RenameAttempted
                | PrivatePublicationPhase::CleanupUncertain
                | PrivatePublicationPhase::WriteFailed
                | PrivatePublicationPhase::PreparationFailed
                | PrivatePublicationPhase::PreservedStaleEvidence
        ) {
            return Ok(PrivateCleanupDisposition::PreservedArtifactEvidence {
                live_file_retained: live,
            });
        }
        let (owned, version) = if let Some(published) = &value.published {
            (&published.owned, &published.version)
        } else if let (Some(stage), Some(version)) = (&value.stage, &value.owned_version) {
            (stage, version)
        } else {
            return Ok(PrivateCleanupDisposition::PreservedArtifactEvidence {
                live_file_retained: live,
            });
        };
        // Ownership originates ONLY from the pre-write creation receipt; a fresh read merely
        // checks that exact held FD and its owned content version have not changed.
        if !owned.identity().matches(owned.as_file())? {
            return Err(FileError::conflict());
        }
        let current = read_file(owned.as_file(), PRIVATE_METADATA_MAX_BYTES)?;
        if &current.version != version {
            return Err(FileError::conflict());
        }
        value.phase = PrivatePublicationPhase::CleanupUncertain;
        private_native::remove(
            &value.parent,
            &owned.0.leaf,
            owned.as_file(),
            PrivateKind::File,
        )?;
        value.phase = PrivatePublicationPhase::CleanupDeletePending;
        Ok(PrivateCleanupDisposition::DeletePending)
    }
}
