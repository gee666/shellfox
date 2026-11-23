//! Trusted native application resources. No network request can construct these.
mod programs;
pub use programs::NativeProgramResource;
mod portable;
pub use portable::portable_install_resources;

use crate::system::{HostSystemServices, NativeSystemBackend};
use anyhow::Result;
use shellfox_ssh::{ProcessRunner, StoragePolicy, SystemProcessRunner};
use std::{path::Path, sync::Arc};

#[derive(Clone)]
pub struct NativeHostProviders {
    system: Arc<NativeSystemBackend>,
}
impl NativeHostProviders {
    pub fn new(system: Arc<NativeSystemBackend>) -> Self {
        Self { system }
    }
    pub fn system_backend(&self) -> &Arc<NativeSystemBackend> {
        &self.system
    }
    pub fn system_services(&self) -> Arc<HostSystemServices> {
        Arc::new(HostSystemServices::new(self.system.clone()))
    }
}

pub(crate) fn ssh_storage_policy(helper: &Path, digest: [u8; 32]) -> Result<StoragePolicy> {
    let helper = NativeProgramResource::from_pinned_file(helper.to_owned(), digest)?;
    Ok(StoragePolicy::with_windows_helper(helper.path())
        .with_process_runner(Arc::new(PermissionsRunner(helper))))
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
