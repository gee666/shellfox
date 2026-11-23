//! Private children of the host data root, kept open from preparation through binding.
use super::*;
use shellfox_files::{
    private::{self, PrivateDir},
    FileService, NativeCommitFence,
};
use std::ffi::OsStr;

pub(super) enum DirectoryRecord {
    Created(CreationHandle),
    Existing(PrivateDir),
}
impl DirectoryRecord {
    pub fn clone_file(&self) -> Result<std::fs::File> {
        match self {
            Self::Created(record) => record.clone_file(),
            Self::Existing(record) => Ok(record.as_file().try_clone()?),
        }
    }
    pub fn verify(&self) -> Result<()> {
        match self {
            Self::Created(record) => record.verify(),
            Self::Existing(record) => Ok(record.verify()?),
        }
    }
}
pub(super) struct Child {
    pub name: String,
    pub record: DirectoryRecord,
}
/// Creates one missing component of the data root path.
pub(super) fn create(
    owner: &Arc<NativeCreationRetention>,
    parent: &Dir,
    name: &OsStr,
) -> Result<CreationHandle> {
    let parent = parent.try_clone()?.into_std_file();
    owner.retain(private::create_app_dir(&parent, name))
}
/// Scope over the named private child, bound to its held handle, not its path.
pub(super) fn bind_control(
    root: &HostRoot,
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
        .context("Private control child missing")?;
    child.record.verify()?;
    let files = FileService::private_scope(child.record.clone_file()?, fence)?;
    root.revalidate()?;
    Ok(files)
}
pub(super) fn prepare_child(
    owner: &Arc<NativeCreationRetention>,
    parent: &Dir,
    name: &str,
) -> Result<Child> {
    let held = parent.try_clone()?.into_std_file();
    let record = match parent.symlink_metadata(name) {
        Ok(before) => {
            safe(&before)?;
            let original = parent.open_dir_nofollow(name)?;
            ensure!(
                same(&before, &original.dir_metadata()?),
                "App data child changed while opening"
            );
            private::repair_app_dir(&held, OsStr::new(name), &original.into_std_file())
                .with_context(|| {
                    format!("Cannot make the Shellfox data directory child private: {name}")
                })?;
            let existing = private::open_app_dir(&held, OsStr::new(name))?;
            let actual = Dir::from_std_file(existing.as_file().try_clone()?).dir_metadata()?;
            ensure!(
                same(&before, &actual),
                "Existing private child changed during verification"
            );
            DirectoryRecord::Existing(existing)
        }
        // An existing entry created in between is a conflict, never adopted.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            DirectoryRecord::Created(owner.retain(private::create_dir(&held, OsStr::new(name)))?)
        }
        Err(error) => return Err(error.into()),
    };
    record.verify()?;
    Ok(Child {
        name: name.to_owned(),
        record,
    })
}
