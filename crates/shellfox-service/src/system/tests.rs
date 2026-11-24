use super::*;
use shellfox_protocol::ErrorCode;
use std::fs;
pub(super) mod support;
use support::Fixture;

#[test]
fn facts_are_read_only() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let services = fixture.services();
    let facts = services.facts(SystemScope::User);
    assert_eq!(facts.home.unwrap().cwd, fixture.root.to_str().unwrap());
    assert!(!facts.cli.unwrap().installed);
    assert!(!facts.explorer.unwrap().installed);
    assert!(!fixture.plan.directory.exists());
    assert!(fixture.opener.0.lock().unwrap().is_empty());
    fixture.put_cli();
    assert!(services.facts(SystemScope::User).cli.unwrap().installed);
}
#[test]
fn windows_integration_belongs_to_the_installer() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let services = fixture.services();
    let facts = services.facts(SystemScope::User);
    assert_eq!(facts.cli.unwrap_err().code, ErrorCode::Unsupported);
    assert_eq!(facts.explorer.unwrap_err().code, ErrorCode::Unsupported);
    for action in [
        SystemAction::SetCli {
            installed: true,
            scope: SystemScope::User,
        },
        SystemAction::SetExplorer {
            installed: true,
            scope: SystemScope::User,
        },
    ] {
        let error = services.perform_user_action(action).unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
    }
    assert!(!fixture.plan.directory.exists());
}
#[test]
fn fixed_prefix_cannot_pin_dedup_ids_or_change_native_intent() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    for flag in [
        "--new-session",
        "--cwd",
        "--new-window",
        "--request-id",
        "--request-id=fixed-id",
        "--host=remote",
        "--open-session",
    ] {
        let mut launch = fixture.resources.desktop_launch().unwrap();
        launch.prefix_arguments = vec![flag.into()];
        let error = resources::validate_desktop_prefix(&launch).unwrap_err();
        assert_eq!(error.code, ErrorCode::Validation, "{flag}");
    }
    let mut launch = fixture.resources.desktop_launch().unwrap();
    launch.prefix_arguments = vec!["--data-dir".into(), fixture.root.to_string_lossy().into()];
    assert!(resources::validate_desktop_prefix(&launch).is_ok());
}
#[test]
fn foreign_cli_files_and_unavailable_policy_are_not_overwritten() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    fs::create_dir(&fixture.plan.directory).unwrap();
    let cli = fixture.plan.directory.join("shellfox");
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
fn only_explicit_validated_directories_and_http_links_reach_injected_opener() {
    let fixture = Fixture::new(SystemPlatform::Linux);
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
mod linux_tests;
mod menu_script_tests;
mod native_owner_tests;
mod native_proof_tests;
mod native_service_tests;
mod native_tests;
mod portable_tests;
mod xml;
