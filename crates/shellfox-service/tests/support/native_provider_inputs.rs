pub use super::support::common::dev_helper::digest_file;

use anyhow::Result;
use shellfox_service::{native_providers::*, system::*};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub fn program() -> Result<NativeProgramResource> {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-test-supervisor"));
    let hash = digest_file(&path)?;
    NativeProgramResource::from_pinned_file(path, hash)
}
pub fn providers(home: &Path) -> (NativeHostProviders, OpenRecorder) {
    let opener = OpenRecorder::default();
    let environment = Arc::new(Environment(home.to_owned()));
    let files = Arc::new(NativeSystemFiles);
    let backend = Arc::new(NativeSystemBackend::new(
        SystemProviders {
            environment: environment.clone(),
            resources: Arc::new(NoInstallation),
            files: files.clone(),
            opener: Arc::new(opener.clone()),
            linux_discovery: Arc::new(NativeSystemLinuxDiscovery { environment, files }),
        },
        SystemInstallPolicy {
            allow_user_install: false,
            allow_open: true,
        },
    ));
    (NativeHostProviders::new(backend), opener)
}
struct Environment(PathBuf);
impl SystemEnvironment for Environment {
    fn platform(&self) -> SystemPlatform {
        if cfg!(windows) {
            SystemPlatform::Windows
        } else if cfg!(target_os = "macos") {
            SystemPlatform::Macos
        } else {
            SystemPlatform::Linux
        }
    }
    fn home(&self) -> SystemResult<PathBuf> {
        Ok(self.0.clone())
    }
    fn variable(&self, _: &str) -> Option<OsString> {
        None
    }
}
struct NoInstallation;
fn no_install<T>() -> SystemResult<T> {
    Err(shellfox_protocol::AppError {
        code: shellfox_protocol::ErrorCode::Unsupported,
        message: "Owned fixture is not a stable installed package".into(),
        retryable: false,
    })
}
impl SystemInstallResources for NoInstallation {
    fn cli_plan(&self, _: SystemScope) -> SystemResult<CliInstallPlan> {
        no_install()
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        no_install()
    }
    fn validate_current(&self) -> SystemResult<()> {
        no_install()
    }
}
#[derive(Clone, Default)]
pub struct OpenRecorder(Arc<Mutex<Vec<SystemOpenTarget>>>);
impl SystemOpener for OpenRecorder {
    fn open(&self, target: SystemOpenTarget) -> SystemResult<()> {
        self.0.lock().expect("fixture recorder").push(target);
        Ok(())
    }
}
impl OpenRecorder {
    pub fn count(&self) -> usize {
        self.0.lock().expect("fixture recorder").len()
    }
}
