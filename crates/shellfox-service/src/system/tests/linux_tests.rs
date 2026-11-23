use super::super::*;
use super::support::Fixture;
use shellfox_protocol::ErrorCode;
use std::fs;

#[test]
fn all_supported_linux_manager_files_are_owned_and_cli_precedes_menus() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let services = HostSystemServices::new(fixture.backend(
        vec![
            FileManager::Nautilus,
            FileManager::Nemo,
            FileManager::Dolphin,
            FileManager::Thunar,
            FileManager::Caja,
        ],
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
    ));
    assert!(
        !services
            .explorer_integration(SystemScope::User)
            .unwrap()
            .installed
    );
    assert!(!fixture.plan.directory.exists());
    services
        .perform_user_action(SystemAction::SetExplorer {
            installed: true,
            scope: SystemScope::User,
        })
        .unwrap();
    assert!(
        services
            .explorer_integration(SystemScope::User)
            .unwrap()
            .installed
    );
    for file in [
        "data/nautilus-python/extensions/shellfox.py",
        "data/nautilus/scripts/Open in Shellfox",
        "data/nemo/actions/shellfox.nemo_action",
        "data/nemo/actions/shellfox-background.nemo_action",
        "data/kio/servicemenus/shellfox.desktop",
        "data/kservices5/ServiceMenus/shellfox.desktop",
        "config/caja/scripts/Open in Shellfox",
    ] {
        let content = fs::read_to_string(fixture.root.join(file)).unwrap();
        assert!(content.contains("# Shellfox/linux-v1"), "{file}");
    }
    services
        .perform_user_action(SystemAction::SetExplorer {
            installed: false,
            scope: SystemScope::User,
        })
        .unwrap();
    assert!(
        services
            .cli_integration(SystemScope::User)
            .unwrap()
            .installed,
        "removing menus does not silently remove CLI"
    );
    assert!(
        !services
            .explorer_integration(SystemScope::User)
            .unwrap()
            .installed
    );
}
#[test]
fn linux_foreign_menu_causes_preflight_failure_before_cli_or_file_writes() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let file = fixture.root.join("data/nemo/actions/shellfox.nemo_action");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"foreign menu").unwrap();
    let services = HostSystemServices::new(fixture.backend(
        vec![FileManager::Nemo],
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
    ));
    assert_eq!(
        services
            .perform_user_action(SystemAction::SetExplorer {
                installed: true,
                scope: SystemScope::User
            })
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
    assert!(!fixture.plan.directory.exists());
    assert_eq!(fs::read(&file).unwrap(), b"foreign menu");
}
#[test]
fn missing_configured_start_contract_is_an_error_not_false_installation() {
    struct NoContract;
    impl SystemInstallResources for NoContract {
        fn cli_plan(&self, _: SystemScope) -> SystemResult<CliInstallPlan> {
            Err(error(ErrorCode::Unsupported, "No stable CLI contract."))
        }
        fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
            Err(error(ErrorCode::Unsupported, "No stable launch contract."))
        }
        fn validate_current(&self) -> SystemResult<()> {
            Err(error(
                ErrorCode::Unsupported,
                "No stable package resources.",
            ))
        }
    }
    let fixture = Fixture::new(SystemPlatform::Linux);
    let mut backend = fixture.backend(vec![FileManager::Thunar], SystemInstallPolicy::default());
    std::sync::Arc::get_mut(&mut backend).unwrap().resources = std::sync::Arc::new(NoContract);
    let facts = HostSystemServices::new(backend).facts(SystemScope::User);
    assert!(facts.home.is_ok());
    assert_eq!(facts.cli.unwrap_err().code, ErrorCode::Unsupported);
    assert_eq!(facts.explorer.unwrap_err().code, ErrorCode::Unsupported);
}
