//! Original-path, handle-relative host-root preparation. Never canonicalize away
//! an alias before inspecting it, and never let endpoint clients create roots.
use super::creation_retention::{CreationHandle, NativeCreationRetention};
use anyhow::{ensure, Context, Result};
use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, Metadata},
};
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};
#[path = "native_host_private.rs"]
mod private;

#[cfg(unix)]
#[path = "native_host_permissions.rs"]
mod permissions;
#[cfg(test)]
#[path = "native_host_path_tests.rs"]
mod tests;
#[cfg(all(test, unix))]
#[path = "native_host_path_unix_tests.rs"]
mod unix_tests;
struct Anchor {
    path: PathBuf,
    handle: Dir,
    identity: Metadata,
    created: Option<CreationHandle>,
}
pub(crate) struct HostRoot {
    path: PathBuf,
    anchors: Vec<Anchor>,
    missing: Vec<OsString>,
    children: Mutex<Vec<private::Child>>,
    creation_owner: Arc<NativeCreationRetention>,
}
fn safe(info: &Metadata) -> Result<()> {
    ensure!(
        info.is_dir() && !info.file_type().is_symlink(),
        "Host path is not an original directory"
    );
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        ensure!(
            info.file_attributes() & 0x400 == 0,
            "Reparse host path rejected"
        );
    }
    Ok(())
}
fn same(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}
impl HostRoot {
    pub fn inspect(input: &Path) -> Result<Self> {
        let path = shellfox_transport::normalize_private_path(input)?;
        ensure!(
            path.parent().is_some(),
            "A drive/filesystem root is not host data"
        );
        let mut parts = path.components();
        let mut base = PathBuf::new();
        match parts.next().context("Missing host path root")? {
            #[cfg(windows)]
            Component::Prefix(prefix) => {
                ensure!(
                    matches!(prefix.kind(), std::path::Prefix::Disk(_)),
                    "Only local drive host roots are supported"
                );
                base.push(prefix.as_os_str());
                ensure!(
                    matches!(parts.next(), Some(Component::RootDir)),
                    "Host root is not absolute"
                );
                base.push(std::path::MAIN_SEPARATOR.to_string());
            }
            Component::RootDir => base.push(std::path::MAIN_SEPARATOR.to_string()),
            _ => anyhow::bail!("Host root is not absolute"),
        }
        let root = Dir::open_ambient_dir(&base, ambient_authority())?;
        let info = root.dir_metadata()?;
        safe(&info)?;
        let mut anchors = vec![Anchor {
            path: base.clone(),
            handle: root,
            identity: info,
            created: None,
        }];
        let mut missing = Vec::new();
        for part in parts {
            let Component::Normal(name) = part else {
                anyhow::bail!("Host path contains a non-original component");
            };
            ensure!(
                anchors.len() + missing.len() < 256,
                "Host path has too many components"
            );
            if !missing.is_empty() {
                missing.push(name.to_owned());
                continue;
            }
            let parent = &anchors.last().context("Missing host anchor")?.handle;
            match parent.symlink_metadata(name) {
                Ok(info) => {
                    safe(&info)?;
                    let handle = parent.open_dir_nofollow(name)?;
                    let opened = handle.dir_metadata()?;
                    safe(&opened)?;
                    ensure!(same(&info, &opened), "Host ancestor changed while opening");
                    base.push(name);
                    anchors.push(Anchor {
                        path: base.clone(),
                        handle,
                        identity: opened,
                        created: None,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.push(name.to_owned())
                }
                Err(error) => return Err(error.into()),
            }
        }
        // This comparison comes AFTER the original no-follow walk. In addition
        // to links it rejects short-name/drive aliases instead of adopting them.
        let canonical = shellfox_transport::normalize_private_path(&std::fs::canonicalize(&base)?)?;
        #[cfg(windows)]
        ensure!(
            canonical
                .to_string_lossy()
                .eq_ignore_ascii_case(&base.to_string_lossy()),
            "Host ancestor is an alias"
        );
        #[cfg(not(windows))]
        ensure!(canonical == base, "Host ancestor is an alias");
        Ok(Self {
            path,
            anchors,
            missing,
            children: Mutex::new(Vec::new()),
            creation_owner: Arc::default(),
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn exists(&self) -> bool {
        self.missing.is_empty()
    }
    pub fn revalidate(&self) -> Result<()> {
        let current = Self::inspect(&self.path)?;
        ensure!(
            current.missing == self.missing && current.anchors.len() == self.anchors.len(),
            "Original host path changed"
        );
        for (a, b) in self.anchors.iter().zip(&current.anchors) {
            if let Some(created) = &a.created {
                created.verify_binding()?;
            }
            ensure!(
                same(&a.identity, &a.handle.dir_metadata()?) && same(&a.identity, &b.identity),
                "Original host ancestor changed"
            );
        }
        Ok(())
    }
    pub fn creation_owner(&self) -> Arc<NativeCreationRetention> {
        self.creation_owner.clone()
    }
    pub fn original_directory_count(&self) -> usize {
        self.anchors.len()
    }
    pub fn original_directories_from(&self, start: usize) -> Result<Vec<std::fs::File>> {
        let anchors = self
            .anchors
            .get(start..)
            .context("Original directory index changed")?;
        // Duplicate the ORIGINAL handles, including their Windows share modes.
        // Never reopen paths or replace this chain with revalidation handles.
        anchors
            .iter()
            .map(|anchor| Ok(anchor.handle.try_clone()?.into_std_file()))
            .collect()
    }
    pub fn bound_root(&self) -> Result<shellfox_transport::BoundOwnershipRoot> {
        ensure!(self.exists(), "Cannot bind a missing root");
        self.revalidate()?;
        let held = self
            .anchors
            .last()
            .context("Missing root anchor")?
            .handle
            .try_clone()?
            .into_std_file();
        Ok(shellfox_transport::BoundOwnershipRoot::from_held(
            self.path.clone(),
            held,
        )?)
    }
    pub fn create_missing(&mut self) -> Result<()> {
        self.revalidate()?;
        // Allocate success retention before a native call can create an object.
        self.anchors.try_reserve(self.missing.len())?;
        let missing = std::mem::take(&mut self.missing);
        for name in missing {
            let parent = self.anchors.last().context("Missing creation anchor")?;
            let created = private::create(&self.creation_owner, &parent.handle, &name)?;
            // The original owner stores the receipt BEFORE any fallible clone.
            let handle = Dir::from_std_file(created.clone_file()?);
            let info = handle.dir_metadata()?;
            safe(&info)?;
            self.anchors.push(Anchor {
                path: parent.path.join(name),
                handle,
                identity: info,
                created: Some(created),
            });
        }
        self.revalidate()
    }
    pub fn bind_native_parent(
        &self,
        issuer: &Arc<shellfox_files::FileService>,
        fence: Arc<dyn shellfox_files::NativeCommitFence>,
    ) -> Result<shellfox_files::NativeChildIssuer> {
        private::bind_parent(self, issuer, fence)
    }
    pub fn bind_native_control(
        &self,
        issuer: &Arc<shellfox_files::FileService>,
        parent: &shellfox_files::NativeChildIssuer,
        fence: Arc<dyn shellfox_files::NativeCommitFence>,
        name: &str,
    ) -> Result<Arc<shellfox_files::FileService>> {
        private::bind_control(self, issuer, parent, fence, name)
    }
    pub fn read_private_child_leaf(
        &self,
        child_name: &str,
        leaf: &std::ffi::OsStr,
        max: usize,
    ) -> Result<shellfox_files::PrivateRead> {
        self.revalidate()?;
        let mut children = self
            .children
            .lock()
            .map_err(|_| anyhow::anyhow!("Private child retention poisoned"))?;
        if !children.iter().any(|child| child.name == child_name) {
            ensure!(
                children.len() < 2,
                "Private child retention exceeds its bound"
            );
            children.try_reserve(1)?;
            let parent = self
                .anchors
                .last()
                .context("Missing original parent")?
                .handle
                .try_clone()?
                .into_std_file();
            let record = shellfox_files::open_existing_private_at(
                &parent,
                std::ffi::OsStr::new(child_name),
                shellfox_files::PrivateKind::Directory,
                shellfox_files::PrivateAccess::ReadOnly,
            )?;
            children.push(private::Child {
                name: child_name.to_owned(),
                record: private::DirectoryRecord::Existing(record),
            });
        }
        let child = children
            .iter()
            .find(|child| child.name == child_name)
            .context("Missing original child")?;
        child.record.verify_binding()?;
        let read = shellfox_files::bounded_private_read_at(&child.record.clone_file()?, leaf, max)?;
        child.record.verify_binding()?;
        self.revalidate()?;
        Ok(read)
    }
    #[cfg(unix)]
    pub fn private_child_file(&self, name: &str) -> Result<std::fs::File> {
        self.revalidate()?;
        let children = self
            .children
            .lock()
            .map_err(|_| anyhow::anyhow!("Private child retention poisoned"))?;
        let child = children
            .iter()
            .find(|child| child.name == name)
            .context("Original private child missing")?;
        child.record.verify_binding()?;
        let file = child.record.clone_file()?;
        child.record.verify_binding()?;
        self.revalidate()?;
        Ok(file)
    }
    pub fn create_child(&self, name: &str) -> Result<()> {
        self.revalidate()?;
        ensure!(
            self.exists() && !name.is_empty() && Path::new(name).components().count() == 1,
            "Invalid private host child"
        );
        let root = &self.anchors.last().context("Missing host root")?.handle;
        let mut children = self
            .children
            .lock()
            .map_err(|_| anyhow::anyhow!("Private child retention poisoned"))?;
        if let Some(child) = children.iter().find(|child| child.name == name) {
            child.record.verify_binding()?;
            return self.revalidate();
        }
        ensure!(
            children.len() < 2,
            "Private child retention exceeds its bound"
        );
        children.try_reserve(1)?;
        let child = private::prepare_child(&self.creation_owner, root, name)?;
        safe(&Dir::from_std_file(child.record.clone_file()?).dir_metadata()?)?;
        children.push(child);
        self.revalidate()
    }
}
