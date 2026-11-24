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
        remove_legacy_terminal_homes(directory);
        let activity = activity::service::ServiceActivity::new()?;
        let retention = retention::Retention::new(activity.now_ms()?);
        Ok(Self {
            host: Host::open(directory)?,
            terminals: HashMap::new(),
            directory: directory.to_owned(),
            supervisor: supervisor.to_owned(),
            next_request: 0,
            grants: grant::custody::GrantCustody::default(),
            grant_recipient: shellfox_protocol::CancellationToken::new(),
            next_viewer_id: 0,
            native: native::NativeState::default(),
            catalog: native::catalog::NativeCatalogState::default(),
            power: power::service::ServicePower::for_data_dir(directory),
            activity,
            retention,
            ownership_lost: shellfox_protocol::CancellationToken::new(),
            stopping: false,
            metadata_closing: false,
            closing: None,
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

/// Earlier 0.10 builds gave each tab generation a private home under
/// `terminal-home/<tab>/<generation>/`. Remove what is left of that layout,
/// directory by directory and only where empty. Links are never followed;
/// anything else, such as shell history, is kept and reported.
fn remove_legacy_terminal_homes(directory: &Path) {
    let root = directory.join("terminal-home");
    if std::fs::symlink_metadata(&root).is_err() {
        return;
    }
    if !prune_legacy_home(&root, 0) {
        eprintln!(
            "Kept {}: it holds files from an earlier Shellfox build.",
            root.display()
        );
    }
}

const LEGACY_STATE: [&str; 10] = [
    "home",
    "config",
    "data",
    "cache",
    "state",
    "temp",
    "pi-agent",
    "pi-sessions",
    "npm-prefix",
    "pnpm-home",
];

/// Depth 0 is terminal-home, 1 a tab, 2 a generation, 3 a state directory.
fn prune_legacy_home(path: &Path, depth: usize) -> bool {
    if security::reject_links(path, true).is_err() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let known = match depth {
            0 | 1 => name
                .to_str()
                .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok()),
            2 => name
                .to_str()
                .is_some_and(|name| LEGACY_STATE.contains(&name)),
            _ => false,
        };
        if known {
            prune_legacy_home(&entry.path(), depth + 1);
        }
    }
    // remove_dir only removes an empty directory.
    std::fs::remove_dir(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_legacy_homes_are_removed_and_files_are_kept() {
        let data = crate::tests::scratch();
        let root = data.path().join("terminal-home");
        let tab = root.join(uuid::Uuid::new_v4().to_string());
        let empty = tab.join(uuid::Uuid::new_v4().to_string());
        for state in LEGACY_STATE {
            std::fs::create_dir_all(empty.join(state)).unwrap();
        }
        remove_legacy_terminal_homes(data.path());
        assert!(!root.exists());

        let used = tab.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(used.join("state")).unwrap();
        std::fs::create_dir_all(used.join("cache")).unwrap();
        std::fs::write(used.join("state/powershell-history"), "dir\n").unwrap();
        let unknown = root.join("not-a-tab");
        std::fs::create_dir_all(&unknown).unwrap();
        remove_legacy_terminal_homes(data.path());
        assert!(used.join("state/powershell-history").is_file());
        assert!(!used.join("cache").exists());
        assert!(unknown.is_dir(), "only the known layout is removed");
    }

    #[cfg(unix)]
    #[test]
    fn links_in_a_legacy_home_are_not_followed() {
        let data = crate::tests::scratch();
        let outside = data.path().join("outside");
        std::fs::create_dir_all(outside.join("home")).unwrap();
        let root = data.path().join("terminal-home");
        let tab = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&tab).unwrap();
        std::os::unix::fs::symlink(&outside, tab.join(uuid::Uuid::new_v4().to_string())).unwrap();
        remove_legacy_terminal_homes(data.path());
        assert!(outside.join("home").is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn junctions_in_a_legacy_home_are_not_followed() {
        let data = crate::tests::scratch();
        let outside = data.path().join("outside");
        std::fs::create_dir_all(outside.join("home")).unwrap();
        let root = data.path().join("terminal-home");
        let tab = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&tab).unwrap();
        let link = tab.join(uuid::Uuid::new_v4().to_string());
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(status.status.success());
        remove_legacy_terminal_homes(data.path());
        assert!(outside.join("home").is_dir());
        // The junction itself is not a Shellfox directory; it stays too.
        assert!(std::fs::symlink_metadata(&link).is_ok());
    }
}
