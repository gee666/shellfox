//! Owned Service construction; root guard keeps lifetime, not VFS authority.
use super::*;
impl Service {
    /// No production path inference. The caller must pass a private directory and
    /// hold the existing native host-ownership helper for this value's lifetime.
    pub(crate) fn open_owned(directory: &Path, supervisor: &Path) -> Result<Self> {
        if !directory.is_absolute()
            || !directory.is_dir()
            || !supervisor.is_absolute()
            || !supervisor.is_file()
        {
            bail!("Explicit absolute private data directory and supervisor executable required.");
        }
        let isolation = directory.join("terminal-home");
        if !isolation.exists() {
            std::fs::create_dir(&isolation)?;
        }
        security::reject_links(&isolation, true)?;
        Ok(Self {
            host: Host::open(directory)?,
            terminals: HashMap::new(),
            isolation,
            supervisor: supervisor.to_owned(),
            next_request: 0,
            grants: grant_custody::GrantCustody::default(),
            grant_recipient: shellfox_protocol::CancellationToken::new(),
            next_viewer_id: 0,
            remote_v2_mailboxes: remote_v2::mailboxes::ObserverMailboxes::default(),
            manager_v2_mailboxes: manager_mailboxes::ManagerMailboxes::default(),
            native: native::NativeState::default(),
            catalog: native_catalog::NativeCatalogState::default(),
            power: power_service::ServicePower::for_data_dir(directory),
            factory_policy: native_sharing::FactoryPolicyState::default(),
            activity: service_activity::ServiceActivity::new()?,
            ownership_lost: shellfox_protocol::CancellationToken::new(),
            stopping: false,
            metadata_closing: false,
            automation: owner_automation::AutomationState::new(supervisor.to_owned()),
            ssh_profiles: None,
            system_native_handler: None,
            event_wake: None,
            runtime_signal: Default::default(),
            runtime_handle: None,
            _root_lifetime: None,
        })
    }
    /// Retains the original producer chain through SQLite close, including
    /// transfer of the entire Service into deferred cleanup. No VFS claim.
    pub(crate) fn open_owned_with_root_lifetime(
        directory: &Path,
        supervisor: &Path,
        lifetime: shellfox_transport::RootLifetimeGuard,
    ) -> Result<Self> {
        let mut service = Self::open_owned(directory, supervisor)?;
        service._root_lifetime = Some(lifetime);
        Ok(service)
    }
}
