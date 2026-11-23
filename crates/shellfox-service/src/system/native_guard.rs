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
    /// Checks ownership before and after the provider call. A loss after the
    /// call can't undo an accepted OS write, so the error never implies rollback.
    fn run<T>(&self, operation: impl FnOnce() -> SystemResult<T>) -> SystemResult<T> {
        self.check()?;
        let result = operation();
        self.check()?;
        result
    }
}
pub(super) struct GuardedSystemFiles {
    pub inner: Arc<dyn SystemFiles>,
    pub guard: SystemNativeGuard,
}
impl SystemFiles for GuardedSystemFiles {
    fn read(&self, path: &Path) -> SystemResult<Option<SystemFile>> {
        self.guard.run(|| self.inner.read(path))
    }
    fn directory(&self, path: &Path) -> SystemResult<bool> {
        self.guard.run(|| self.inner.directory(path))
    }
    fn program(&self, path: &Path) -> SystemResult<bool> {
        self.guard.run(|| self.inner.program(path))
    }
    fn canonical_directory(&self, path: &Path) -> SystemResult<PathBuf> {
        self.guard.run(|| self.inner.canonical_directory(path))
    }
    fn replace(
        &self,
        path: &Path,
        expected: Option<&SystemFile>,
        bytes: Option<&[u8]>,
        executable: bool,
    ) -> SystemResult<()> {
        self.guard
            .run(|| self.inner.replace(path, expected, bytes, executable))
    }
}
pub(super) struct GuardedSystemRegistry {
    pub inner: Arc<dyn SystemRegistry>,
    pub guard: SystemNativeGuard,
}
impl SystemRegistry for GuardedSystemRegistry {
    fn read(&self, slot: RegistrySlot) -> SystemResult<RegistryState> {
        self.guard.run(|| self.inner.read(slot))
    }
    fn compare_replace(
        &self,
        slot: RegistrySlot,
        expected: &RegistryState,
        desired: Option<&RegistryState>,
    ) -> SystemResult<()> {
        self.guard
            .run(|| self.inner.compare_replace(slot, expected, desired))
    }
}
pub(super) struct GuardedSystemResources {
    pub inner: Arc<dyn SystemInstallResources>,
    pub guard: SystemNativeGuard,
}
impl SystemInstallResources for GuardedSystemResources {
    fn cli_plan(&self, scope: SystemScope) -> SystemResult<CliInstallPlan> {
        self.guard.run(|| self.inner.cli_plan(scope))
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        self.guard.run(|| self.inner.desktop_launch())
    }
    fn validate_current(&self) -> SystemResult<()> {
        self.guard.run(|| self.inner.validate_current())
    }
}
