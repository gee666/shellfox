//! Shared resource-input builder. Inputs are validated by the trusted native
//! producer, never inferred from the running executable or a renderer request.
use shellfox_service::{
    native_providers::NativeHostProviders,
    system::{NativeSystemBackend, SystemInstallPolicy, SystemInstallResources},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct ApplicationProviderResources {
    installation: Arc<dyn SystemInstallResources>,
    policy: SystemInstallPolicy,
}
impl ApplicationProviderResources {
    /// Consume only the existing portable format with an EXTERNAL trusted build
    /// digest. No native-bundle identity coercion or runtime self-hash fallback.
    pub fn from_portable_manifest(
        path: &std::path::Path,
        expected_digest: [u8; 32],
        policy: SystemInstallPolicy,
    ) -> anyhow::Result<Self> {
        let installation =
            shellfox_service::native_providers::portable_install_resources(path, expected_digest)?;
        Ok(Self::new(installation, policy))
    }
    /// Stable package producers supply ConfiguredSystemResources here. Private
    /// acceptance resources must not masquerade as an installed package.
    pub fn new(installation: Arc<dyn SystemInstallResources>, policy: SystemInstallPolicy) -> Self {
        Self {
            installation,
            policy,
        }
    }
    pub fn build(&self, interactive_session: bool) -> NativeHostProviders {
        let system = Arc::new(NativeSystemBackend::native(
            self.installation.clone(),
            self.policy,
            interactive_session,
        ));
        NativeHostProviders::new(system)
    }
}
impl std::fmt::Debug for ApplicationProviderResources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApplicationProviderResources")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "native_app_providers_tests.rs"]
mod tests;
