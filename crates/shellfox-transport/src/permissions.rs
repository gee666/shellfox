#[cfg(windows)]
use crate::offline::OfflineCommand;
use crate::{normalize_private_path, offline_scope::OfflineScope, HelperError, VerifiedHelper};
#[cfg(windows)]
use serde::Deserialize;
use std::path::Path;
#[cfg(windows)]
use std::time::Duration;
#[derive(Clone, Copy, Debug)]
pub enum PrivatePathKind {
    File,
    Directory,
}
#[derive(Clone)]
pub struct PermissionVerifier {
    executable: VerifiedHelper,
    scope: Option<OfflineScope>,
}
impl PermissionVerifier {
    pub fn new(executable: VerifiedHelper) -> Self {
        Self {
            executable,
            scope: None,
        }
    }
    pub(crate) fn scoped(executable: VerifiedHelper, scope: OfflineScope) -> Self {
        Self {
            executable,
            scope: Some(scope),
        }
    }
    pub async fn check(&self, path: &Path, kind: PrivatePathKind) -> Result<(), HelperError> {
        self.run(path, kind, false).await
    }
    pub async fn protect(&self, path: &Path, kind: PrivatePathKind) -> Result<(), HelperError> {
        self.run(path, kind, true).await
    }
    async fn run(
        &self,
        path: &Path,
        kind: PrivatePathKind,
        protect: bool,
    ) -> Result<(), HelperError> {
        let path = normalize_private_path(path)?;
        let path = path.as_path();
        #[cfg(windows)]
        {
            let kind = match kind {
                PrivatePathKind::File => "file",
                PrivatePathKind::Directory => "directory",
            };
            let action = if protect { "protect" } else { "check" };
            let local;
            let scope = if let Some(scope) = &self.scope {
                scope
            } else {
                local = OfflineScope::new(self.executable.clone(), false)?;
                &local
            };
            let mut command = OfflineCommand::spawn_in(
                scope,
                &[
                    Path::new("--private-permissions"),
                    path,
                    Path::new(kind),
                    Path::new(action),
                ],
                protect,
            )?;
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum PermissionResult {
                Success(Success),
                Failure(Failure),
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Success {
                ok: bool,
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Failure {
                error: String,
            }
            let result = tokio::time::timeout(Duration::from_secs(8), async {
                let response: PermissionResult = serde_json::from_slice(&command.line().await?)
                    .map_err(|_| HelperError::Protocol)?;
                match response {
                    PermissionResult::Success(success) if success.ok => command.expect_exit().await,
                    PermissionResult::Failure(failure) => {
                        command.wait_original().await?;
                        Err(match failure.error.as_str() {
                            "Invalid private permissions arguments" => HelperError::InvalidRequest,
                            "Unsafe private permissions" => HelperError::PermissionDenied,
                            _ => HelperError::Command,
                        })
                    }
                    _ => Err(HelperError::Protocol),
                }
            })
            .await
            .map_err(|_| HelperError::Deadline)
            .and_then(|result| result);
            // Every timeout/error reaches original termination AND wait. A wait
            // failure is returned with controller retained in the same scope.
            command.terminate().await?;
            result
        }
        #[cfg(unix)]
        {
            let _ = &self.executable;
            if let Some(scope) = &self.scope {
                scope.native(protect, || unix_permissions(path, kind, protect))
            } else {
                unix_permissions(path, kind, protect)
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (path, kind, protect);
            Err(HelperError::Configuration)
        }
    }
}

#[cfg(unix)]
fn unix_permissions(path: &Path, kind: PrivatePathKind, protect: bool) -> Result<(), HelperError> {
    use rustix::{
        fs::{self, FileType, Mode, OFlags},
        process::getuid,
    };
    let directory = matches!(kind, PrivatePathKind::Directory);
    let flags = OFlags::RDONLY
        | OFlags::NOFOLLOW
        | OFlags::CLOEXEC
        | OFlags::NONBLOCK
        | if directory {
            OFlags::DIRECTORY
        } else {
            OFlags::empty()
        };
    let parent = crate::BoundOwnershipRoot::capture_current(
        path.parent().ok_or(HelperError::InvalidRequest)?,
    )?;
    let name = path.file_name().ok_or(HelperError::InvalidRequest)?;
    let fd = fs::openat(parent.held_file(), name, flags, Mode::empty())
        .map_err(|_| HelperError::PermissionDenied)?;
    let info = fs::fstat(&fd).map_err(|_| HelperError::PermissionDenied)?;
    let expected = if directory {
        FileType::Directory
    } else {
        FileType::RegularFile
    };
    if FileType::from_raw_mode(info.st_mode) != expected
        || info.st_uid != getuid().as_raw()
        || (!directory && info.st_nlink != 1)
    {
        return Err(HelperError::PermissionDenied);
    }
    #[cfg(not(target_os = "macos"))]
    crate::unix_acl::verify_acl(&fd)?;
    if protect {
        fs::fchmod(
            &fd,
            Mode::from_raw_mode(if directory { 0o700 } else { 0o600 }),
        )
        .map_err(|_| HelperError::PermissionDenied)?;
        #[cfg(target_os = "macos")]
        crate::unix_acl::tighten_held_acl(&fd)?;
    }
    #[cfg(target_os = "macos")]
    crate::unix_acl::verify_acl(&fd)?;
    let info = fs::fstat(&fd).map_err(|_| HelperError::PermissionDenied)?;
    if info.st_mode & if directory { 0o077 } else { 0o177 } != 0 {
        return Err(HelperError::PermissionDenied);
    }
    Ok(())
}
