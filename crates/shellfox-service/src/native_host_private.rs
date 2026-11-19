//! Native directory records retain the shared producer's actual parent/child.
use super::*;
use shellfox_files::{
    FileService, NativeChildIssuer, NativeCommitFence, PreparedPrivateDirectoryRef, PrivateAccess,
    PrivateKind, VerifiedPrivate,
};
use std::ffi::OsStr;

pub(super) enum DirectoryRecord {
    Created(CreationHandle),
    Existing(VerifiedPrivate),
}
impl DirectoryRecord {
    pub fn clone_file(&self) -> Result<std::fs::File> {
        match self {
            Self::Created(record) => record.clone_file(),
            Self::Existing(record) => Ok(record.as_file().try_clone()?),
        }
    }
    pub fn verify_binding(&self) -> Result<()> {
        match self {
            Self::Created(record) => record.verify_binding()?,
            Self::Existing(record) => record.verify_binding()?,
        }
        Ok(())
    }
}
pub(super) struct Child {
    pub name: String,
    pub record: DirectoryRecord,
}
pub(super) fn create(
    owner: &Arc<NativeCreationRetention>,
    parent: &Dir,
    name: &OsStr,
) -> Result<CreationHandle> {
    #[cfg(windows)]
    let create = shellfox_files::create_app_directory_at;
    #[cfg(unix)]
    let create = shellfox_files::create_private_directory_at;
    create_retained(owner, parent, name, create)
}
fn create_retained(
    owner: &Arc<NativeCreationRetention>,
    parent: &Dir,
    name: &OsStr,
    create: impl FnOnce(
        &std::fs::File,
        &OsStr,
    ) -> std::result::Result<
        shellfox_files::CreatedPrivate,
        shellfox_files::PrivateCreationFailure,
    >,
) -> Result<CreationHandle> {
    let parent = parent.try_clone()?.into_std_file();
    let reservation = owner.reserve(&parent, name)?;
    reservation.finish(create(&parent, name))
}
pub(super) fn bind_parent(
    root: &HostRoot,
    issuer: &Arc<FileService>,
    fence: Arc<dyn NativeCommitFence>,
) -> Result<NativeChildIssuer> {
    root.revalidate()?;
    let final_anchor = root.anchors.last().context("Missing original root")?;
    let result = if let Some(created) = &final_anchor.created {
        created.with_created(|record| {
            Ok(issuer
                .native_child_issuer(PreparedPrivateDirectoryRef::Created(record), fence.clone())?)
        })?
    } else {
        // Verify relative to the ORIGINAL held parent, then match the SAME
        // original Go-bound root. The receipt is not captured from a path.
        let parent = root
            .anchors
            .get(
                root.anchors
                    .len()
                    .checked_sub(2)
                    .context("Root parent missing")?,
            )
            .context("Root parent missing")?;
        let parent = parent.handle.try_clone()?.into_std_file();
        let leaf = root.path.file_name().context("Root leaf missing")?;
        #[cfg(windows)]
        let original = shellfox_files::open_app_directory_at(&parent, leaf)?;
        #[cfg(unix)]
        let original = shellfox_files::open_existing_private_at(
            &parent,
            leaf,
            PrivateKind::Directory,
            PrivateAccess::ReadOnly,
        )?;
        let held = final_anchor.handle.try_clone()?.into_std_file();
        ensure!(
            original.identity().matches(&held)?,
            "Prepared root differs from original Go root"
        );
        let rw = issuer.reopen_native_directory_rw(&original, fence.clone())?;
        issuer.native_child_issuer(PreparedPrivateDirectoryRef::Verified(&rw), fence.clone())?
    };
    root.revalidate()?;
    Ok(result)
}
pub(super) fn bind_control(
    root: &HostRoot,
    issuer: &Arc<FileService>,
    parent: &NativeChildIssuer,
    fence: Arc<dyn NativeCommitFence>,
    name: &str,
) -> Result<Arc<FileService>> {
    root.revalidate()?;
    let children = root
        .children
        .lock()
        .map_err(|_| anyhow::anyhow!("Private child retention poisoned"))?;
    let child = children
        .iter()
        .find(|child| child.name == name)
        .context("Original private control child missing")?;
    let result = match &child.record {
        DirectoryRecord::Created(created) => {
            created.with_created(|record| Ok(parent.created_directory_root(record)?))?
        }
        DirectoryRecord::Existing(original) => {
            // ReadOnly is never relabelled. Reopen under the native RW issuer,
            // match the full original ID, then derive from NEW sealed provenance.
            let rw = issuer.reopen_native_directory_rw(original, fence)?;
            parent.verified_directory_root(&rw)?
        }
    };
    root.revalidate()?;
    Ok(result)
}
pub(super) fn prepare_child(
    owner: &Arc<NativeCreationRetention>,
    parent: &Dir,
    name: &str,
) -> Result<Child> {
    let record = match parent.symlink_metadata(name) {
        Ok(before) => {
            safe(&before)?;
            let held = parent.try_clone()?.into_std_file();
            let original = parent.open_dir_nofollow(name)?;
            ensure!(
                same(&before, &original.dir_metadata()?),
                "App data child changed while opening"
            );
            shellfox_files::repair_app_directory_at(
                &held,
                OsStr::new(name),
                &original.into_std_file(),
            )
            .with_context(|| {
                format!("Cannot make the Shellfox data directory child private: {name}")
            })?;
            let existing = shellfox_files::open_existing_private_at(
                &held,
                OsStr::new(name),
                PrivateKind::Directory,
                PrivateAccess::ReadOnly,
            )?;
            let actual = Dir::from_std_file(existing.as_file().try_clone()?).dir_metadata()?;
            ensure!(
                same(&before, &actual),
                "Existing private child changed during verification"
            );
            DirectoryRecord::Existing(existing)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Exclusive-create collisions are errors, never an adoption fallback.
            DirectoryRecord::Created(create_retained(
                owner,
                parent,
                OsStr::new(name),
                shellfox_files::create_private_directory_at,
            )?)
        }
        Err(error) => return Err(error.into()),
    };
    record.verify_binding()?;
    Ok(Child {
        name: name.to_owned(),
        record,
    })
}
