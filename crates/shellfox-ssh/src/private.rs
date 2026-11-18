use crate::{
    private_platform as platform, Error, ProcessRequest, ProcessRunner, Result, SystemProcessRunner,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, DirBuilder, OpenOptions},
};
#[path = "private_identity.rs"]
mod identity;
#[path = "private_lock.rs"]
mod locking;
use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;
use zeroize::Zeroizing;

/// Host configuration only. Never accept a permissions executable from a remote request.
#[derive(Clone)]
pub struct StoragePolicy {
    helper: Option<PathBuf>,
    runner: Arc<dyn ProcessRunner>,
}
impl Default for StoragePolicy {
    fn default() -> Self {
        Self {
            helper: None,
            runner: Arc::new(SystemProcessRunner),
        }
    }
}
impl StoragePolicy {
    pub fn with_windows_helper(path: impl Into<PathBuf>) -> Self {
        Self {
            helper: Some(path.into()),
            ..Self::default()
        }
    }
    pub fn with_process_runner(mut self, runner: Arc<dyn ProcessRunner>) -> Self {
        self.runner = runner;
        self
    }
    fn permissions(&self, path: &Path, directory: bool, protect: bool) -> Result<()> {
        if !cfg!(windows) {
            return Ok(());
        }
        #[cfg(windows)]
        let native_path = identity::native_helper_path(path)?;
        #[cfg(windows)]
        let path = native_path.as_path();
        let helper = self.helper.as_ref().ok_or(Error::PermissionsHelper)?;
        if !helper.is_absolute() || !helper.is_file() {
            return Err(Error::PermissionsHelper);
        }
        let result = self
            .runner
            .run(&ProcessRequest {
                executable: helper.clone(),
                args: vec![
                    "--private-permissions".into(),
                    path.as_os_str().into(),
                    if directory { "directory" } else { "file" }.into(),
                    if protect { "protect" } else { "check" }.into(),
                ],
                timeout: Duration::from_secs(8),
                max_output: 1024,
            })
            .map_err(|_| Error::PermissionsHelper)?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            ok: bool,
        }
        if !result.success
            || !result.stderr.is_empty()
            || !serde_json::from_slice::<Reply>(&result.stdout).is_ok_and(|r| r.ok)
        {
            return Err(Error::PermissionsHelper);
        }
        Ok(())
    }
}

/// An explicit private directory. Its parent must already exist and be trusted by the host.
/// No home-directory discovery and no automatic import from production SSH storage.
pub struct PrivateDirectory {
    path: PathBuf,
    dir: Dir,
    handle: File,
    policy: StoragePolicy,
    serial: Mutex<()>,
}
impl PrivateDirectory {
    pub fn open(path: impl AsRef<Path>, policy: StoragePolicy) -> Result<Self> {
        let path = path.as_ref();
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(Error::Validation);
        }
        let parent = path.parent().ok_or(Error::Validation)?;
        let name = path.file_name().ok_or(Error::Validation)?;
        let parent = Dir::open_ambient_dir(parent, ambient_authority())?;
        let builder = DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use cap_std::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        let created = match parent.create_dir_with(name, &builder) {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(_) => return Err(Error::Storage),
        };
        // Reject final-component links/reparse points before protecting an empty new directory.
        let dir = parent
            .open_dir_nofollow(name)
            .map_err(|_| Error::Permissions)?;
        policy.permissions(path, true, created)?;
        let handle = platform::directory_handle(path)?;
        let pinned = dir.try_clone()?.into_std_file();
        if !platform::same(&handle, &pinned)? {
            return Err(Error::Permissions);
        }
        let result = Self {
            path: path.to_owned(),
            dir,
            handle,
            policy,
            serial: Mutex::new(()),
        };
        result.check()?;
        Ok(result)
    }
    /// Host-only private IPC path. Remote clients must not receive this capability.
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn file_path(&self, name: &str) -> Result<PathBuf> {
        Self::name(name)?;
        self.check()?;
        Ok(self.path.join(name))
    }
    fn check(&self) -> Result<()> {
        let current = platform::directory_handle(&self.path)?;
        if !platform::same(&self.handle, &current)? {
            return Err(Error::Permissions);
        }
        platform::check(&self.handle, true)?;
        self.policy.permissions(&self.path, true, false)
    }
    fn name(name: &str) -> Result<()> {
        if name.is_empty()
            || name.len() > 100
            || name.starts_with('.')
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
        {
            return Err(Error::Validation);
        }
        Ok(())
    }
    fn open_file(&self, name: &str, create: bool) -> Result<File> {
        Self::name(name)?;
        self.check()?;
        if !create {
            let metadata = self.dir.symlink_metadata(name)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(Error::Permissions);
            }
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(create)
            .create_new(create)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NONBLOCK);
        }
        let file = self.dir.open_with(name, &options)?.into_std();
        platform::check(&file, false)?;
        self.policy
            .permissions(&self.path.join(name), false, create)?;
        platform::check(&file, false)?;
        Ok(file)
    }
    pub(crate) fn read(&self, name: &str, limit: usize) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Self::name(name)?;
        self.check()?;
        match self.dir.symlink_metadata(name) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(Error::Storage),
            Ok(_) => {}
        }
        let file = self.open_file(name, false)?;
        if file.metadata()?.len() > limit as u64 {
            return Err(Error::Limit);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::Limit);
        }
        Ok(Some(bytes))
    }
    pub(crate) fn atomic_write(&self, name: &str, bytes: &[u8], limit: usize) -> Result<()> {
        Self::name(name)?;
        if bytes.len() > limit {
            return Err(Error::Limit);
        }
        self.check()?;
        // Never overwrite an unsafe pre-existing target, even if rename would unlink it safely.
        if self.dir.symlink_metadata(name).is_ok() {
            self.open_file(name, false)?;
        }
        let stage = format!("ssh-stage-{}.tmp", Uuid::new_v4());
        let result = (|| {
            let mut file = self.open_file(&stage, true)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            self.check()?;
            self.dir.rename(&stage, &self.dir, name)?;
            platform::sync_directory(&self.handle)?;
            Ok(())
        })();
        match self.dir.remove_file(&stage) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Storage),
        }
        result
    }
    pub(crate) fn remove(&self, name: &str) -> Result<()> {
        Self::name(name)?;
        self.check()?;
        if self
            .dir
            .symlink_metadata(name)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok(());
        }
        self.open_file(name, false)?;
        self.dir.remove_file(name)?;
        platform::sync_directory(&self.handle)
    }
}
