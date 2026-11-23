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
            grants: grant::custody::GrantCustody::default(),
            grant_recipient: shellfox_protocol::CancellationToken::new(),
            next_viewer_id: 0,
            native: native::NativeState::default(),
            catalog: native::catalog::NativeCatalogState::default(),
            power: power::service::ServicePower::for_data_dir(directory),
            activity: activity::service::ServiceActivity::new()?,
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
