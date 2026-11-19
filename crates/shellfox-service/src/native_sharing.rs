//! Owner-native eligibility policy. This is not privilege, peer permission or wire data.
#[cfg(test)]
#[path = "native_sharing_tests.rs"]
mod tests;
use crate::*;
use shellfox_runtime::Identity;
use shellfox_terminal::{PtyConfig, ShellProfileId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeFactoryPolicy {
    #[default]
    Disabled,
    ExistingManagerStandardCatalog,
    /// Trusted explicit Native0 source configuration, never incoming metadata.
    #[cfg(feature = "experimental-shellfox-native")]
    ExistingManagerNative0Catalog {
        configuration: shellfox_ghostty_vt::extension::RestorationConfig,
        limits: shellfox_ghostty_vt::extension::SnapshotLimits,
    },
}
impl NativeFactoryPolicy {
    fn permits_catalog(self) -> bool {
        match self {
            Self::Disabled => false,
            Self::ExistingManagerStandardCatalog => true,
            #[cfg(feature = "experimental-shellfox-native")]
            Self::ExistingManagerNative0Catalog { .. } => true,
        }
    }
    fn validate(self) -> Result<()> {
        #[cfg(feature = "experimental-shellfox-native")]
        if let Self::ExistingManagerNative0Catalog {
            configuration,
            limits,
        } = self
        {
            shellfox_runtime::Native0RuntimeConfig::new(configuration, limits)?;
        }
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct FactoryPolicyState {
    policy: NativeFactoryPolicy,
    revision: u64,
}
/// The launch path creates this before moving config into Runtime. Not Clone,
/// serde, a profile-name setter, or a reconstruction from later preferences.
pub(crate) struct CapturedCatalogOrigin {
    profile: Option<ShellProfileId>,
}
pub(crate) fn capture_catalog_origin(config: &PtyConfig) -> CapturedCatalogOrigin {
    let profile = if config.command.is_none() {
        config
            .profile
            .filter(|profile| !matches!(profile, ShellProfileId::Wsl))
    } else {
        None
    };
    CapturedCatalogOrigin { profile }
}
pub(crate) struct SourceShareRecord {
    identity: Identity,
    catalog_profile: Option<ShellProfileId>,
    eligible_revision: Option<u64>,
}
impl SourceShareRecord {
    pub fn local_only(identity: Identity) -> Self {
        Self {
            identity,
            catalog_profile: None,
            eligible_revision: None,
        }
    }
    pub fn captured(identity: Identity, origin: CapturedCatalogOrigin) -> Self {
        Self {
            identity,
            catalog_profile: origin.profile,
            eligible_revision: None,
        }
    }
    fn activate(&mut self, identity: Identity, started: bool, policy: &FactoryPolicyState) {
        self.eligible_revision = if self.identity == identity
            && started
            && self.catalog_profile.is_some()
            && policy.policy.permits_catalog()
        {
            Some(policy.revision)
        } else {
            None
        };
    }
}
impl Service {
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) fn catalog_native0_creation(
        &self,
        origin: &CapturedCatalogOrigin,
    ) -> Result<Option<shellfox_runtime::Native0RuntimeConfig>> {
        if origin.profile.is_none() {
            return Ok(None);
        }
        self.native0_creation()
    }
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) fn native0_creation(&self) -> Result<Option<shellfox_runtime::Native0RuntimeConfig>> {
        match self.factory_policy.policy {
            NativeFactoryPolicy::ExistingManagerNative0Catalog {
                configuration,
                limits,
            } => Ok(Some(shellfox_runtime::Native0RuntimeConfig::new(
                configuration,
                limits,
            )?)),
            _ => Ok(None),
        }
    }
    /// Eligibility epoch ONLY. The backend must independently require sealed
    /// actual KnownNormal/purpose/stage, exact binding and current peer permission.
    pub(crate) fn observer_delivery_policy(&self, identity: Identity) -> Option<u64> {
        if self.stopping {
            return None;
        }
        let terminal = self.terminals.get(&identity.tab_id)?;
        if terminal.runtime.identity() != identity
            || terminal.source_share.identity != identity
            || !matches!(terminal.startup, Some(Ok(())))
            || !self.factory_policy.policy.permits_catalog()
        {
            return None;
        }
        if terminal.runtime.source_kind() == shellfox_runtime::SourceKind::ExternalBytes {
            return self.remote_automation_exit_code(terminal).map(|_| self.factory_policy.revision);
        }
        terminal
            .source_share
            .eligible_revision
            .filter(|revision| *revision == self.factory_policy.revision)
    }
    pub(crate) fn activate_observer_factory_policy(&mut self, tab: TabId) {
        if let Some(terminal) = self.terminals.get_mut(&tab) {
            terminal.source_share.activate(
                terminal.runtime.identity(),
                matches!(terminal.startup, Some(Ok(()))),
                &self.factory_policy,
            );
        }
    }
    pub(crate) fn disable_observer_factory_policy(&mut self) -> Result<()> {
        self.factory_policy.policy = NativeFactoryPolicy::Disabled;
        let identities: Vec<_> = self
            .terminals
            .values_mut()
            .map(|terminal| {
                terminal.source_share.eligible_revision = None;
                terminal.runtime.identity()
            })
            .collect();
        // Invalidate EVERY source's queued delivery/recovery authority before
        // the first bounded Runtime detach/release is even attempted.
        for identity in identities {
            self.remote_v2_mailboxes.revoke_source(identity);
            self.manager_v2_mailboxes.revoke_source(identity);
        }
        self.pump_remote_observer_retirements()
    }
    /// Trusted native configuration only. Disable/purge old authority before
    /// bounded Unobserve. Failed retirement leaves every source disabled for retry.
    pub(crate) fn install_observer_factory_policy(
        &mut self,
        policy: NativeFactoryPolicy,
    ) -> Result<()> {
        self.assert_accepting()?;
        self.disable_observer_factory_policy()?;
        policy.validate()?;
        self.factory_policy.revision = self
            .factory_policy
            .revision
            .checked_add(1)
            .context("Native factory policy revision exhausted.")?;
        self.factory_policy.policy = policy;
        let tabs: Vec<_> = self.terminals.keys().copied().collect();
        for tab in tabs {
            self.activate_observer_factory_policy(tab);
        }
        Ok(())
    }
}
