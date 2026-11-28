// No test starts Service, a shell or UAC.
use shellfox_core::{
    ErrorCode, Result, SessionId, Settings, SettingsAdapter, SettingsPatch, SettingsShell,
};
use shellfox_host::{Host, SettingsRuntime};
use shellfox_protocol::CancellationToken;
use shellfox_service::settings_runtime::*;
use std::{path::PathBuf, sync::Arc, time::Duration};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("settings-runtime-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn host(&self) -> Host {
        Host::open(&self.0).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct SeedMetadata;
impl SettingsRuntime for SeedMetadata {
    fn normalize(&self, _: &Settings, candidate: Settings) -> Result<Settings> {
        candidate.validate()?;
        Ok(candidate)
    }
}
#[test]
fn lost_native_ownership_rejects_before_work_and_preserves_all_persisted_state() {
    let scratch = Scratch::new();
    let host = scratch.host();
    let lost = CancellationToken::new();
    host.install_settings_runtime(Arc::new(SettingsRuntimeAdapter::new(lost.clone())))
        .unwrap();
    let settings = host.global_settings().unwrap();
    let snapshot = host.snapshot().unwrap();
    lost.cancel();
    assert!(host
        .save_global_settings(SettingsPatch {
            accent_color: Some("#123456".into()),
            ..Default::default()
        })
        .is_err());
    assert_eq!(host.global_settings().unwrap(), settings);
    assert_eq!(host.snapshot().unwrap(), snapshot);
}
#[test]
fn opaque_unknown_profile_is_preserved_on_autosave_but_switch_never_falls_back() {
    let scratch = Scratch::new();
    let host = scratch.host();
    host.install_settings_runtime(Arc::new(SeedMetadata))
        .unwrap();
    let opaque = "wsl:Opaque distro; $(not-a-command)";
    host.save_global_settings(SettingsPatch {
        terminal_profile_id: Some(Some(opaque.into())),
        shell_id: Some(SettingsShell::Wsl),
        shell_executable: Some(Some("/metadata/unavailable".into())),
        adapter_id: Some(SettingsAdapter::EmbeddedPty),
        ..Default::default()
    })
    .unwrap();
    host.install_settings_runtime(Arc::new(SettingsRuntimeAdapter::new(
        CancellationToken::new(),
    )))
    .unwrap();
    let saved = host
        .save_global_settings(SettingsPatch {
            history_page_size: Some(23),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(saved.terminal_profile_id, Some(Some(opaque.into())));
    assert_eq!(saved.shell_executable, Some("/metadata/unavailable".into()));
    assert_eq!(
        host.snapshot()
            .unwrap()
            .preferences
            .default_profile_id
            .as_str(),
        opaque
    );
    let snapshot = host.snapshot().unwrap();
    for profile in [
        "evil;run-command",
        "shell:/unverified/bin/sh",
        "wsl:Different distro",
    ] {
        assert_eq!(
            host.save_global_settings(SettingsPatch {
                terminal_profile_id: Some(Some(profile.into())),
                ..Default::default()
            })
            .unwrap_err()
            .code,
            ErrorCode::Unsupported
        );
        assert_eq!(host.global_settings().unwrap(), saved);
        assert_eq!(host.snapshot().unwrap(), snapshot);
    }
}
#[test]
fn known_native_ownership_gates_are_not_replaced_with_an_available_default() {
    let scratch = Scratch::new();
    let host = scratch.host();
    host.install_settings_runtime(Arc::new(SettingsRuntimeAdapter::new(
        CancellationToken::new(),
    )))
    .unwrap();
    let before = host.global_settings().unwrap();
    let snapshot = host.snapshot().unwrap();
    for profile in ["wsl", "wsl:Ubuntu", "shell:/bin/bash"] {
        assert_eq!(
            host.save_global_settings(SettingsPatch {
                terminal_profile_id: Some(Some(profile.into())),
                ..Default::default()
            })
            .unwrap_err()
            .code,
            ErrorCode::Unsupported
        );
        assert_eq!(host.global_settings().unwrap(), before);
        assert_eq!(host.snapshot().unwrap(), snapshot);
    }
}
#[test]
fn settings_validation_budget_is_bounded() {
    for timeout in [
        Duration::ZERO,
        SETTINGS_PROBE_TIMEOUT + Duration::from_secs(1),
    ] {
        assert!(SettingsRuntimeAdapter::new(CancellationToken::new())
            .with_probe_timeout(timeout)
            .is_err());
    }
}
#[cfg(windows)]
#[test]
fn actual_windows_catalog_changes_normalize_without_launching() {
    use shellfox_terminal::{discover_shell_profiles, shell_profile, ShellProfileId};
    let scratch = Scratch::new();
    let host = scratch.host();
    host.install_settings_runtime(Arc::new(SettingsRuntimeAdapter::new(
        CancellationToken::new(),
    )))
    .unwrap();
    let mut count = 0;
    for discovered in discover_shell_profiles()
        .unwrap()
        .into_iter()
        .filter(|p| p.unavailable_reason().is_none())
    {
        let checked = shell_profile(discovered.id()).unwrap();
        let expected_shell = match checked.id() {
            ShellProfileId::Pwsh => SettingsShell::Pwsh,
            ShellProfileId::WindowsPowerShell => SettingsShell::WindowsPowershell,
            ShellProfileId::Cmd => SettingsShell::LoginShell,
            _ => continue,
        };
        let saved = host
            .save_global_settings(SettingsPatch {
                terminal_profile_id: Some(Some(checked.id().as_str().into())),
                shell_executable: Some(None),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(saved.adapter_id, SettingsAdapter::EmbeddedPty);
        assert_eq!(saved.shell_id, expected_shell);
        assert_eq!(
            saved.shell_executable.as_deref(),
            checked.executable().unwrap().to_str()
        );
        assert_eq!(
            host.snapshot()
                .unwrap()
                .preferences
                .default_profile_id
                .as_str(),
            checked.id().as_str()
        );
        count += 1;
    }
    assert!(
        count > 0,
        "Windows must discover its actual standard shells"
    );
    let before = host.snapshot().unwrap();
    assert!(host
        .save_global_settings(SettingsPatch {
            shell_executable: Some(Some("C:\\untrusted-settings-runtime.exe".into())),
            ..Default::default()
        })
        .is_err());
    assert_eq!(host.snapshot().unwrap(), before);
}
#[test]
fn saving_the_profile_a_null_selection_implies_is_no_switch() {
    // `settings read` prints the implied profile; scripts save it back.
    let scratch = Scratch::new();
    let host = scratch.host();
    host.install_settings_runtime(Arc::new(SettingsRuntimeAdapter::new(
        CancellationToken::new(),
    )))
    .unwrap();
    assert_eq!(host.global_settings().unwrap().terminal_profile_id, None);
    let implied = host.snapshot().unwrap().preferences.default_profile_id;
    host.save_global_settings(SettingsPatch {
        terminal_profile_id: Some(Some(implied.to_string())),
        ..Default::default()
    })
    .unwrap();
}
