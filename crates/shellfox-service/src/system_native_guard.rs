use super::{
    error, CliInstallPlan, DesktopLaunch, RegistrySlot, RegistryState, SystemFile, SystemFiles,
    SystemInstallResources, SystemRegistry, SystemResult, SystemScope,
};
use shellfox_protocol::{CancellationToken, ErrorCode};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub(super) struct SystemNativeGuard {
    pub lost: CancellationToken,
    pub admissions: Arc<super::native_authority::SystemNativeAdmissions>,
}
impl SystemNativeGuard {
    pub fn check(&self) -> SystemResult<()> {
        if self.lost.is_cancelled() || self.admissions.is_closing() {
            return Err(error(ErrorCode::NativeUnavailable,"Native system ownership is lost. No action was retried; any accepted external change must be reconciled."));
        }
        Ok(())
    }
}
pub(super) struct GuardedSystemFiles {
    pub inner: Arc<dyn SystemFiles>,
    pub guard: SystemNativeGuard,
}
impl SystemFiles for GuardedSystemFiles {
    fn read(&self, path: &Path) -> SystemResult<Option<SystemFile>> {
        self.guard.check()?;
        let result = self.inner.read(path);
        self.guard.check()?;
        result
    }
    fn directory(&self, path: &Path) -> SystemResult<bool> {
        self.guard.check()?;
        let result = self.inner.directory(path);
        self.guard.check()?;
        result
    }
    fn program(&self, path: &Path) -> SystemResult<bool> {
        self.guard.check()?;
        let result = self.inner.program(path);
        self.guard.check()?;
        result
    }
    fn canonical_directory(&self, path: &Path) -> SystemResult<PathBuf> {
        self.guard.check()?;
        let result = self.inner.canonical_directory(path);
        self.guard.check()?;
        result
    }
    fn replace(
        &self,
        path: &Path,
        expected: Option<&SystemFile>,
        bytes: Option<&[u8]>,
        executable: bool,
    ) -> SystemResult<()> {
        // Checks between every actual provider write. A cancellation cannot undo
        // an already accepted OS write, so errors never imply rollback/retry.
        self.guard.check()?;
        let result = self.inner.replace(path, expected, bytes, executable);
        self.guard.check()?;
        result
    }
}
pub(super) struct GuardedSystemRegistry {
    pub inner: Arc<dyn SystemRegistry>,
    pub guard: SystemNativeGuard,
}
impl SystemRegistry for GuardedSystemRegistry {
    fn read(&self, slot: RegistrySlot) -> SystemResult<RegistryState> {
        self.guard.check()?;
        let result = self.inner.read(slot);
        self.guard.check()?;
        result
    }
    fn compare_replace(
        &self,
        slot: RegistrySlot,
        expected: &RegistryState,
        desired: Option<&RegistryState>,
    ) -> SystemResult<()> {
        self.guard.check()?;
        let result = self.inner.compare_replace(slot, expected, desired);
        self.guard.check()?;
        result
    }
}
pub(super) struct GuardedSystemResources {
    pub inner: Arc<dyn SystemInstallResources>,
    pub guard: SystemNativeGuard,
}
impl SystemInstallResources for GuardedSystemResources {
    fn cli_plan(&self, scope: SystemScope) -> SystemResult<CliInstallPlan> {
        self.guard.check()?;
        let result = self.inner.cli_plan(scope);
        self.guard.check()?;
        result
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        self.guard.check()?;
        let result = self.inner.desktop_launch();
        self.guard.check()?;
        result
    }
    fn validate_current(&self) -> SystemResult<()> {
        self.guard.check()?;
        let result = self.inner.validate_current();
        self.guard.check()?;
        result
    }
}
