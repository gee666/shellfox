use super::*;
use shellfox_protocol::ErrorCode;
use std::{fs, sync::atomic::Ordering};
pub(super) mod support;
use support::Fixture;

#[test]
fn facts_are_read_only_and_errors_are_not_missing_installations() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let services = fixture.services();
    let facts = services.facts(SystemScope::User);
    assert_eq!(facts.home.unwrap().cwd, fixture.root.to_str().unwrap());
    assert!(!facts.cli.unwrap().installed);
    assert!(!facts.explorer.unwrap().installed);
    assert!(!fixture.plan.directory.exists());
    assert_eq!(fixture.registry.writes.load(Ordering::SeqCst), 0);
    assert!(fixture.opener.0.lock().unwrap().is_empty());
    fixture.put_cli();
    *fixture.registry.fail.lock().unwrap() = true;
    let facts = services.facts(SystemScope::User);
    assert!(facts.home.is_ok());
    assert_eq!(facts.cli.unwrap_err().code, ErrorCode::NativeUnavailable);
    assert_eq!(
        facts.explorer.unwrap_err().code,
        ErrorCode::NativeUnavailable
    );
    assert_eq!(fixture.registry.writes.load(Ordering::SeqCst), 0);
}
#[test]
fn explicit_user_cli_actions_preserve_other_path_entries_and_value_kind() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let services = fixture.services();
    let mut path = RegistryState {
        exists: true,
        ..Default::default()
    };
    path.values.insert(
        RegistryField::Path,
        RegistryText {
            value: "unrelated;%UNKNOWN_VARIABLE%".into(),
            kind: RegistryStringKind::Plain,
        },
    );
    fixture
        .registry
        .values
        .lock()
        .unwrap()
        .insert(RegistrySlot::UserPath, path);
    let SystemResponse::Cli(state) = services
        .perform_user_action(SystemAction::SetCli {
            installed: true,
            scope: SystemScope::User,
        })
        .unwrap()
    else {
        panic!("CLI response")
    };
    assert!(state.installed);
    let state = fixture.registry.read(RegistrySlot::UserPath).unwrap();
    assert_eq!(
        state.values[&RegistryField::Path].kind,
        RegistryStringKind::Plain
    );
    assert!(state
        .text(RegistryField::Path)
        .unwrap()
        .starts_with("unrelated;%UNKNOWN_VARIABLE%;"));
    services
        .perform_user_action(SystemAction::SetCli {
            installed: false,
            scope: SystemScope::User,
        })
        .unwrap();
    assert_eq!(
        fixture
            .registry
            .read(RegistrySlot::UserPath)
            .unwrap()
            .text(RegistryField::Path),
        Some("unrelated;%UNKNOWN_VARIABLE%")
    );
    assert!(!fixture.plan.marker.exists());
}
#[test]
fn foreign_cli_files_and_unavailable_policy_are_not_overwritten() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    fs::create_dir(&fixture.plan.directory).unwrap();
    let cli = fixture.plan.directory.join("shellfox.cmd");
    fs::write(&cli, b"foreign command").unwrap();
    let services = fixture.services();
    assert_eq!(
        services
            .perform_user_action(SystemAction::SetCli {
                installed: true,
                scope: SystemScope::User
            })
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
    assert_eq!(fs::read(cli).unwrap(), b"foreign command");
    assert!(!fixture.plan.marker.exists());
    assert_eq!(fixture.registry.writes.load(Ordering::SeqCst), 0);
    let denied =
        HostSystemServices::new(fixture.backend(Vec::new(), SystemInstallPolicy::default()));
    assert_eq!(
        denied
            .perform_user_action(SystemAction::SetCli {
                installed: true,
                scope: SystemScope::User
            })
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        services
            .perform_user_action(SystemAction::SetCli {
                installed: true,
                scope: SystemScope::System
            })
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}
#[test]
fn windows_explorer_uses_owned_typed_slots_and_root_safe_arguments() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let services = fixture.services();
    let SystemResponse::Explorer(state) = services
        .perform_user_action(SystemAction::SetExplorer {
            installed: true,
            scope: SystemScope::User,
        })
        .unwrap()
    else {
        panic!("Explorer response")
    };
    assert!(state.installed);
    let folder = fixture
        .registry
        .read(RegistrySlot::Explorer {
            scope: SystemScope::User,
            background: false,
        })
        .unwrap();
    let background = fixture
        .registry
        .read(RegistrySlot::Explorer {
            scope: SystemScope::User,
            background: true,
        })
        .unwrap();
    assert!(folder
        .text(RegistryField::Command)
        .unwrap()
        .ends_with("--new-session --cwd \"%1\\.\""));
    assert!(background
        .text(RegistryField::Command)
        .unwrap()
        .ends_with("--new-session --cwd \"%V\\.\""));
    assert_eq!(
        folder.text(RegistryField::Owner),
        Some(resources::EXPLORER_OWNER)
    );
    services
        .perform_user_action(SystemAction::SetExplorer {
            installed: false,
            scope: SystemScope::User,
        })
        .unwrap();
    assert!(
        !services
            .explorer_integration(SystemScope::User)
            .unwrap()
            .installed
    );
}
#[test]
fn foreign_explorer_key_prevents_all_registry_writes() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let mut foreign = RegistryState {
        exists: true,
        ..Default::default()
    };
    foreign.values.insert(
        RegistryField::Owner,
        RegistryText {
            value: "another program".into(),
            kind: RegistryStringKind::Plain,
        },
    );
    fixture.registry.values.lock().unwrap().insert(
        RegistrySlot::Explorer {
            scope: SystemScope::User,
            background: true,
        },
        foreign,
    );
    assert_eq!(
        fixture
            .services()
            .perform_user_action(SystemAction::SetExplorer {
                installed: true,
                scope: SystemScope::User
            })
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
    assert_eq!(fixture.registry.writes.load(Ordering::SeqCst), 0);
}
#[test]
fn only_explicit_validated_directories_and_http_links_reach_injected_opener() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let services = fixture.services();
    let file = fixture.root.join("ordinary.txt");
    fs::write(&file, b"never execute an associated program").unwrap();
    assert_eq!(
        services
            .perform_user_action(SystemAction::OpenDirectory { path: file })
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    for bad in [
        "javascript:alert(1)",
        "file:///tmp/example",
        "ftp://example.test/a",
        "relative",
        "https://example.test/\nargument",
    ] {
        assert!(services
            .perform_user_action(SystemAction::OpenWeb { url: bad.into() })
            .is_err());
    }
    assert!(fixture.opener.0.lock().unwrap().is_empty());
    services
        .perform_user_action(SystemAction::OpenDirectory {
            path: fixture.root.clone(),
        })
        .unwrap();
    services
        .perform_user_action(SystemAction::OpenWeb {
            url: "https://example.test/a b".into(),
        })
        .unwrap();
    let opened = fixture.opener.0.lock().unwrap();
    assert_eq!(opened.len(), 2);
    assert!(matches!(&opened[1],SystemOpenTarget::Web(url)if url=="https://example.test/a%20b"));
}
#[test]
fn staged_files_detect_revision_changes_and_preserve_permissions() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let file = fixture.root.join("owned.xml");
    fs::write(&file, b"original").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let before = fixture.files.read(&file).unwrap().unwrap();
    fixture
        .files
        .replace(&file, Some(&before), Some(b"updated"), false)
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let before = fixture.files.read(&file).unwrap().unwrap();
    fs::write(&file, b"concurrent user edit").unwrap();
    assert_eq!(
        fixture
            .files
            .replace(&file, Some(&before), Some(b"must not clobber"), false)
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    assert_eq!(fs::read(file).unwrap(), b"concurrent user edit");
}
mod deb_tests;
mod file_tests;
mod launch_tests;
mod linux_tests;
mod native_owner_tests;
mod native_proof_tests;
mod native_service_tests;
mod native_tests;
mod registry_tests;
mod xml;
