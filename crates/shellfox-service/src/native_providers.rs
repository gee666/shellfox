//! Trusted native application resources. No network request can construct these.
#[path = "native_provider_programs.rs"]
mod programs;
pub use programs::{NativeProgramResource, NativeSshPrograms};
#[path = "native_provider_portable.rs"]
mod portable;
pub use portable::portable_install_resources;

use crate::{
    ssh,
    system::{HostSystemServices, NativeSystemBackend},
};
use anyhow::{bail, ensure, Result};
use shellfox_ssh::{PrivateDirectory, ProcessRunner, StoragePolicy, SystemProcessRunner};
use std::{path::Path, sync::Arc};

#[derive(Clone)]
pub struct NativeHostProviders {
    system: Arc<NativeSystemBackend>,
    pub(crate) ssh: Option<NativeSshResources>,
}
impl NativeHostProviders {
    pub fn new(system: Arc<NativeSystemBackend>) -> Self {
        Self { system, ssh: None }
    }
    pub fn with_ssh(mut self, ssh: NativeSshResources) -> Self {
        self.ssh = Some(ssh);
        self
    }
    pub fn system_backend(&self) -> &Arc<NativeSystemBackend> {
        &self.system
    }
    pub fn system_services(&self) -> Arc<HostSystemServices> {
        Arc::new(HostSystemServices::new(self.system.clone()))
    }
}

#[derive(Clone)]
pub struct NativeSshResources {
    programs: Arc<NativeSshPrograms>,
    permissions_helper: Option<NativeProgramResource>,
    imports: Arc<dyn ssh::NativePlatform>,
}
impl NativeSshResources {
    pub fn new(
        programs: NativeSshPrograms,
        permissions_helper: Option<NativeProgramResource>,
    ) -> Result<Self> {
        ensure!(
            !cfg!(windows) || permissions_helper.is_some(),
            "Windows SSH storage requires its validated private-permissions helper."
        );
        Ok(Self {
            programs: Arc::new(programs),
            permissions_helper,
            imports: Arc::new(UnsupportedImports),
        })
    }
    /// Explicit discovery adapter only. Construction never calls it.
    pub fn with_imports(mut self, imports: Arc<dyn ssh::NativePlatform>) -> Self {
        self.imports = imports;
        self
    }
    /// Bootstrap calls this only after its original owner and actor exist.
    /// It can block on bounded permission checks; never run it on a UI/actor thread.
    pub(crate) fn materialize(&self, directory: &Path) -> Result<ssh::NativeSshInstall> {
        self.programs.ssh().validate_current()?;
        self.programs.keyscan().validate_current()?;
        self.programs.askpass_cli().validate_current()?;
        let policy = match &self.permissions_helper {
            Some(helper) => StoragePolicy::with_windows_helper(helper.path())
                .with_process_runner(Arc::new(PermissionsRunner(helper.clone()))),
            None => StoragePolicy::default(),
        };
        let directory = Arc::new(PrivateDirectory::open(directory, policy)?);
        let adapter = Arc::new(shellfox_ssh::system::SystemSshAdapter::new(
            shellfox_ssh::system::SshPrograms {
                ssh: self.programs.ssh().path().to_owned(),
                keyscan: self.programs.keyscan().path().to_owned(),
            },
            Arc::new(SystemProcessRunner),
            directory.clone(),
        )?);
        Ok(ssh::NativeSshInstall {
            directory,
            adapter,
            imports: self.imports.clone(),
            askpass: self.programs.askpass_cli().path().to_owned(),
            program: self.programs.clone(),
        })
    }
}
struct UnsupportedImports;
impl ssh::NativePlatform for UnsupportedImports {
    fn import_sessions(&self) -> Result<Vec<shellfox_ssh::ImportSession>> {
        bail!("Native SSH import discovery is not configured on this host; no personal profiles were scanned.")
    }
}
struct PermissionsRunner(NativeProgramResource);
impl ProcessRunner for PermissionsRunner {
    fn run(
        &self,
        request: &shellfox_ssh::ProcessRequest,
    ) -> shellfox_ssh::Result<shellfox_ssh::ProcessOutput> {
        let args = &request.args;
        if request.executable != self.0.path()
            || args.len() != 4
            || args[0] != "--private-permissions"
            || !matches!(args[2].to_str(), Some("directory" | "file"))
            || !matches!(args[3].to_str(), Some("protect" | "check"))
        {
            return Err(shellfox_ssh::Error::PermissionsHelper);
        }
        self.0
            .validate_current()
            .map_err(|_| shellfox_ssh::Error::PermissionsHelper)?;
        let result = SystemProcessRunner.run(request);
        self.0
            .validate_current()
            .map_err(|_| shellfox_ssh::Error::PermissionsHelper)?;
        result
    }
}
