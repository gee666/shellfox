use shellfox_host::*;
use std::{path::PathBuf, sync::Arc};
fn profiles() -> Vec<SettingsProfile> {
    vec![
        SettingsProfile {
            id: "pwsh".into(),
            shell_id: SettingsShell::Pwsh,
            executable: "C:\\Program Files\\PowerShell\\7\\pwsh.exe".into(),
            available: true,
            elevation: false,
        },
        SettingsProfile {
            id: "wsl:Ubuntu (dev)".into(),
            shell_id: SettingsShell::Wsl,
            executable: "C:\\Windows\\System32\\wsl.exe".into(),
            available: true,
            elevation: false,
        },
        SettingsProfile {
            id: "wsl:Unavailable distro".into(),
            shell_id: SettingsShell::Wsl,
            executable: "C:\\Windows\\System32\\wsl.exe".into(),
            available: false,
            elevation: false,
        },
        SettingsProfile {
            id: "administrator-powershell".into(),
            shell_id: SettingsShell::WindowsPowershell,
            executable: "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".into(),
            available: true,
            elevation: true,
        },
    ]
}
#[test]
fn explicit_discovered_profile_owns_executable_and_adapter_on_switch() {
    let previous = Settings {
        shell_executable: Some(profiles()[0].executable.clone()),
        terminal_profile_id: Some(Some("pwsh".into())),
        ..Default::default()
    };
    let mut candidate = previous.clone();
    candidate.terminal_profile_id = Some(Some("wsl:Ubuntu (dev)".into()));
    let normalized =
        normalize_settings_profile(&previous, candidate, &profiles(), Some("pwsh")).unwrap();
    assert_eq!(
        normalized.terminal_profile_id,
        Some(Some("wsl:Ubuntu (dev)".into()))
    );
    assert_eq!(normalized.shell_id, SettingsShell::Wsl);
    assert_eq!(normalized.adapter_id, SettingsAdapter::EmbeddedPty);
    assert_eq!(
        normalized.shell_executable,
        Some(profiles()[1].executable.clone())
    );
    assert_eq!(
        normalized
            .project_preferences(&Preferences::default())
            .unwrap()
            .default_profile_id
            .as_str(),
        "wsl:Ubuntu (dev)"
    );
}
#[test]
fn arbitrary_executables_unavailable_elevation_and_unknown_opaque_ids_fail_closed() {
    let previous = Settings::default();
    for id in [
        "wsl:Unavailable distro",
        "administrator-powershell",
        "evil;run-command",
        "shell:/bin/arbitrary",
    ] {
        // Valid opaque METADATA must not be mistaken for an executable capability.
        let metadata = ProfileId::new(id).unwrap();
        assert_eq!(metadata.as_str(), id);
        let mut candidate = previous.clone();
        candidate.terminal_profile_id = Some(Some(id.into()));
        assert_eq!(
            normalize_settings_profile(&previous, candidate, &profiles(), Some("pwsh"))
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
    }
    let mut candidate = previous.clone();
    candidate.terminal_profile_id = Some(Some("pwsh".into()));
    candidate.shell_executable = Some("C:\\untrusted.exe".into());
    assert!(normalize_settings_profile(&previous, candidate, &profiles(), Some("pwsh")).is_err());
}
struct Discovery;
impl SettingsRuntime for Discovery {
    fn normalize(&self, previous: &Settings, candidate: Settings) -> Result<Settings> {
        if candidate.python_path != previous.python_path {
            return Err(Error::new(ErrorCode::Validation, "Python is not usable."));
        }
        normalize_settings_profile(previous, candidate, &profiles(), Some("pwsh"))
    }
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("settings-profile-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn trusted_runtime_port_canonicalizes_atomically_and_failed_validation_preserves_revision() {
    let dir = Scratch::new();
    let host = Host::open(&dir.0).unwrap();
    host.install_settings_runtime(Arc::new(Discovery)).unwrap();
    let saved = host
        .save_global_settings(SettingsPatch {
            terminal_profile_id: Some(Some("wsl:Ubuntu (dev)".into())),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(saved.shell_id, SettingsShell::Wsl);
    assert_eq!(
        host.snapshot()
            .unwrap()
            .preferences
            .default_profile_id
            .as_str(),
        "wsl:Ubuntu (dev)"
    );
    let snapshot = host.snapshot().unwrap();
    for patch in [
        SettingsPatch {
            terminal_profile_id: Some(Some("evil;run-command".into())),
            ..Default::default()
        },
        SettingsPatch {
            python_path: Some(Some("/unusable/python".into())),
            accent_color: Some("#123456".into()),
            ..Default::default()
        },
    ] {
        assert!(host.save_global_settings(patch).is_err());
        assert_eq!(host.global_settings().unwrap(), saved);
        assert_eq!(host.snapshot().unwrap(), snapshot);
    }
}
