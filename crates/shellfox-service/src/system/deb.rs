//! The .deb layout, as in 0.9: the app in /usr/lib/shellfox and the
//! package-owned /usr/bin/shellfox launcher. File-manager menus run
//! `/usr/bin/shellfox start <dir>`.
use super::{
    error, resources::*, NativeSystemBackend, SystemEnvironment, SystemInstallPolicy,
    SystemProviders, SystemResult, SystemScope,
};
use shellfox_protocol::ErrorCode;
use std::{path::PathBuf, sync::Arc};

pub struct DebPackage {
    lib: PathBuf,
    bin: PathBuf,
}
impl DebPackage {
    /// The installed package, when this executable is its desktop binary.
    pub fn current() -> Option<Self> {
        let package = Self::at("/usr/lib/shellfox".into(), "/usr/bin".into());
        let exe = std::env::current_exe().ok()?;
        (exe == package.desktop()).then_some(package)
    }
    pub fn at(lib: PathBuf, bin: PathBuf) -> Self {
        Self { lib, bin }
    }
    fn desktop(&self) -> PathBuf {
        self.lib.join("shellfox")
    }
    fn launcher(&self) -> PathBuf {
        self.bin.join("shellfox")
    }
    /// Per-user menus for this package, against the given HOME/XDG environment.
    pub fn backend(self, environment: Arc<dyn SystemEnvironment>) -> NativeSystemBackend {
        user_backend(Arc::new(self), environment)
    }
}
impl SystemInstallResources for DebPackage {
    fn cli_plan(&self, scope: SystemScope) -> SystemResult<CliInstallPlan> {
        if scope == SystemScope::System {
            return Err(error(
                ErrorCode::Unsupported,
                "The package owns the system CLI.",
            ));
        }
        // The package owns the launcher, so the installed file is the plan;
        // a launcher without Shellfox's marker fails plan validation.
        let launcher = self.launcher();
        let bytes = std::fs::read(&launcher).map_err(|_| {
            error(
                ErrorCode::NativeUnavailable,
                "The packaged shellfox launcher is missing.",
            )
        })?;
        Ok(CliInstallPlan {
            directory: self.bin.clone(),
            files: vec![ManagedFile {
                path: launcher.clone(),
                bytes,
                executable: true,
            }],
            marker: launcher,
            package_managed: true,
        })
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        Ok(DesktopLaunch {
            executable: self.desktop(),
            prefix_arguments: Vec::new(),
            new_session_contract: true,
            disposition: DesktopLaunchDisposition::ExistingWindow,
            cli_start: Some(self.launcher()),
        })
    }
    fn validate_current(&self) -> SystemResult<()> {
        if self.desktop().is_file() && self.launcher().is_file() {
            Ok(())
        } else {
            Err(error(
                ErrorCode::NativeUnavailable,
                "The Shellfox package is incomplete.",
            ))
        }
    }
}

/// A backend that installs per-user integration for an installed layout.
pub(super) fn user_backend(
    resources: Arc<dyn SystemInstallResources>,
    environment: Arc<dyn SystemEnvironment>,
) -> NativeSystemBackend {
    let files: Arc<dyn super::SystemFiles> = Arc::new(super::NativeSystemFiles);
    NativeSystemBackend::new(
        SystemProviders {
            linux_discovery: Arc::new(super::NativeSystemLinuxDiscovery {
                environment: environment.clone(),
                files: files.clone(),
            }),
            environment,
            resources,
            files,
            registry: Arc::new(super::NativeSystemRegistry),
            opener: Arc::new(super::NativeSystemOpener {
                interactive_session: false,
            }),
        },
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
    )
}
