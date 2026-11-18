//! Live user-stage authority: original user capability, held parent and actual creator FD.
//! Persisted journal paths are diagnostics and never reconstruct this owner after restart.
#[cfg(unix)]
use crate::{content, platform};
use crate::{paths::Target, upload_private, FileError, FileService, HandleIdentity, Result};
use cap_std::fs::{Dir, File};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StageState {
    Planned,
    Creating,
    Owned,
    Publishing,
    PublishedUnconfirmed,
    Published,
    Preserved,
}
struct Capture {
    file: Option<std::fs::File>,
    identity: Option<HandleIdentity>,
    state: StageState,
    #[cfg(windows)]
    attempt: Option<crate::user_stage_windows::StageAttempt>,
}
pub(crate) struct UserStageCustody {
    capability: Arc<FileService>,
    selector: String,
    target: Target,
    parent_id: HandleIdentity,
    name: PathBuf,
    capture: Mutex<Capture>,
}
impl UserStageCustody {
    pub fn reserve(
        capability: Arc<FileService>,
        selector: &str,
        target: &Target,
        name: &Path,
    ) -> Result<Arc<Self>> {
        capability.authorized(true)?;
        capability.recheck_parent(selector, target)?;
        let parent_id = HandleIdentity::from_directory(&target.parent)?;
        #[cfg(unix)]
        crate::private_verify::parent(&target.parent.try_clone()?.into_std_file())?;
        Ok(Arc::new(Self {
            capability,
            selector: selector.into(),
            target: Target {
                parent: target.parent.try_clone()?,
                name: target.name.clone(),
                absolute: target.absolute.clone(),
            },
            parent_id,
            name: name.into(),
            capture: Mutex::new(Capture {
                file: None,
                identity: None,
                state: StageState::Planned,
                #[cfg(windows)]
                attempt: None,
            }),
        }))
    }
    pub fn create(&self, check: &dyn Fn() -> Result<()>) -> Result<File> {
        let _mutation = FileService::mutation_lock()?;
        self.check()?;
        check()?;
        let mut capture = self.capture.lock().map_err(|_| FileError::denied())?;
        if capture.state != StageState::Planned {
            return Err(FileError::denied());
        }
        let interrupted = FileError::new(
            crate::ErrorCode::StorageFailed,
            "User-stage creation interrupted; original custody and intent retained.",
        );
        capture.state = StageState::Creating;
        // Both guards and the original capture are outside this catch. Returned FD is installed
        // before permission/identity/authorization postchecks; lower errors remain possible intent.
        let result = catch_unwind(AssertUnwindSafe(|| {
            #[cfg(windows)]
            {
                capture.attempt = Some(crate::user_stage_windows::StageAttempt::prepare(
                    self.target.parent.try_clone()?.into_std_file(),
                    self.name.as_os_str(),
                )?);
                let Capture { file, attempt, .. } = &mut *capture;
                attempt
                    .as_mut()
                    .ok_or_else(FileError::denied)?
                    .create_into(file, &|| {
                        self.check()?;
                        check()
                    })?;
            }
            #[cfg(unix)]
            {
                capture.file = Some(
                    self.target
                        .parent
                        .open_with(&self.name, &platform::stage_options())?
                        .into_std(),
                );
            }
            let file = capture.file.as_ref().ok_or_else(FileError::denied)?;
            let working = File::from_std(file.try_clone()?);
            upload_private::protect_file(&working)?;
            let identity = HandleIdentity::from_file(file)?;
            capture.identity = Some(identity);
            capture.state = StageState::Owned;
            self.check()?;
            check()?;
            Ok(working)
        }));
        match result {
            Ok(value) => value,
            Err(_) => Err(interrupted),
        }
    }
    pub fn check(&self) -> Result<()> {
        self.capability.authorized(true)?;
        self.capability
            .recheck_parent(&self.selector, &self.target)?;
        if HandleIdentity::from_directory(&self.target.parent)? != self.parent_id {
            return Err(FileError::conflict());
        }
        Ok(())
    }
    #[cfg(windows)]
    pub fn publish(&self, replace: bool, check: &dyn Fn() -> Result<()>) -> Result<()> {
        self.check()?;
        let capture = self.capture.lock().map_err(|_| FileError::denied())?;
        if capture.state != StageState::Publishing {
            return Err(FileError::denied());
        }
        let file = capture.file.as_ref().ok_or_else(FileError::denied)?;
        if capture.identity.as_ref() != Some(&HandleIdentity::from_file(file)?) {
            return Err(FileError::conflict());
        }
        crate::platform::rename_user_stage_held_checked(
            file,
            &self.target.parent.try_clone()?.into_std_file(),
            self.target.name.as_os_str(),
            replace,
            &|| {
                self.check()?;
                check()
            },
        )
    }
    pub fn publishing(&self) -> Result<()> {
        let mut c = self.capture.lock().map_err(|_| FileError::denied())?;
        if c.state != StageState::Owned {
            return Err(FileError::denied());
        }
        c.state = StageState::Publishing;
        Ok(())
    }
    pub fn visible(&self) -> Result<()> {
        let mut c = self.capture.lock().map_err(|_| FileError::denied())?;
        c.state = StageState::PublishedUnconfirmed;
        Ok(())
    }
    pub fn published(&self) -> Result<()> {
        let mut c = self.capture.lock().map_err(|_| FileError::denied())?;
        c.state = StageState::Published;
        Ok(())
    }
    /// Original receipt only, usable after capability revocation. No post-restart adoption.
    pub fn dispose(&self) -> Result<bool> {
        let mut c = self.capture.lock().map_err(|_| FileError::denied())?;
        let removed = match c.state {
            StageState::Planned => true,
            StageState::Published => true, // intended user data, never delete the published target
            StageState::Owned => {
                if let (Some(file), Some(identity)) = (&c.file, &c.identity) {
                    if HandleIdentity::from_directory(&self.target.parent)? != self.parent_id
                        || HandleIdentity::from_file(file)? != *identity
                    {
                        return Err(FileError::conflict());
                    }
                    remove_owned(
                        &self.target.parent,
                        &self.name,
                        &File::from_std(file.try_clone()?),
                    )?
                } else {
                    false
                }
            }
            _ => false,
        };
        c.state = StageState::Preserved;
        c.file = None;
        Ok(removed)
    }
    pub fn preserve(&self) {
        let mut c = self
            .capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        c.state = StageState::Preserved;
        c.file = None;
    }
}
#[cfg(test)]
#[path = "user_stage_custody_tests.rs"]
mod tests;
fn remove_owned(parent: &Dir, name: &Path, original: &File) -> Result<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
        };
        let _ = (parent, name);
        let held = original;
        let value = FILE_DISPOSITION_INFO { DeleteFile: 1 };
        // SAFETY: DELETE handle is pinned and matched to the ORIGINAL creator FD, not read-derived ownership.
        if unsafe {
            SetFileInformationByHandle(
                held.as_raw_handle(),
                FileDispositionInfo,
                (&value as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(true)
    }
    #[cfg(unix)]
    {
        // Existing strict parent trust excludes other-account namespace writers. Hostile same-account
        // or unsupported ACL/fence cases are NOT inferred safe; preserve instead.
        if crate::private_verify::parent(&parent.try_clone()?.into_std_file()).is_err() {
            return Ok(false);
        }
        let held = match content::open_regular(parent, name) {
            Ok(file) => file,
            Err(error) if error.code == crate::ErrorCode::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if !content::same_held_file(&held, original)? {
            return Err(FileError::conflict());
        }
        parent.remove_file(name)?;
        Ok(true)
    }
}
