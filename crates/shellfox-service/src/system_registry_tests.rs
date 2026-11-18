use super::super::*;
use super::support::Fixture;
use shellfox_protocol::ErrorCode;
use std::sync::atomic::Ordering;

#[test]
fn legacy_keys_are_only_removed_by_explicit_action_and_only_when_owned() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    for (background, owner) in [
        (false, "PiManager/native-v1"),
        (true, "foreign application"),
    ] {
        let mut value = RegistryState {
            exists: true,
            ..Default::default()
        };
        value.values.insert(
            RegistryField::Owner,
            RegistryText {
                value: owner.into(),
                kind: RegistryStringKind::Plain,
            },
        );
        fixture.registry.values.lock().unwrap().insert(
            RegistrySlot::LegacyExplorer {
                scope: SystemScope::User,
                background,
            },
            value,
        );
    }
    let services = fixture.services();
    services.explorer_integration(SystemScope::User).unwrap();
    assert_eq!(fixture.registry.writes.load(Ordering::SeqCst), 0);
    assert!(
        fixture
            .registry
            .read(RegistrySlot::LegacyExplorer {
                scope: SystemScope::User,
                background: false
            })
            .unwrap()
            .exists
    );
    services
        .perform_user_action(SystemAction::SetExplorer {
            installed: true,
            scope: SystemScope::User,
        })
        .unwrap();
    assert!(
        !fixture
            .registry
            .read(RegistrySlot::LegacyExplorer {
                scope: SystemScope::User,
                background: false
            })
            .unwrap()
            .exists
    );
    assert_eq!(
        fixture
            .registry
            .read(RegistrySlot::LegacyExplorer {
                scope: SystemScope::User,
                background: true
            })
            .unwrap()
            .text(RegistryField::Owner),
        Some("foreign application")
    );
}
#[test]
fn empty_persisted_path_does_not_introduce_a_current_directory_entry() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let mut state = RegistryState {
        exists: true,
        ..Default::default()
    };
    state.values.insert(
        RegistryField::Path,
        RegistryText {
            value: String::new(),
            kind: RegistryStringKind::Expandable,
        },
    );
    fixture
        .registry
        .values
        .lock()
        .unwrap()
        .insert(RegistrySlot::UserPath, state);
    fixture
        .services()
        .perform_user_action(SystemAction::SetCli {
            installed: true,
            scope: SystemScope::User,
        })
        .unwrap();
    assert_eq!(
        fixture
            .registry
            .read(RegistrySlot::UserPath)
            .unwrap()
            .text(RegistryField::Path),
        fixture.plan.directory.to_str()
    );
}
#[test]
fn registry_revision_conflicts_do_not_change_the_target() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let previous = fixture.registry.read(RegistrySlot::UserPath).unwrap();
    let mut concurrent = RegistryState {
        exists: true,
        ..Default::default()
    };
    concurrent.values.insert(
        RegistryField::Path,
        RegistryText {
            value: "concurrent user value".into(),
            kind: RegistryStringKind::Plain,
        },
    );
    fixture
        .registry
        .values
        .lock()
        .unwrap()
        .insert(RegistrySlot::UserPath, concurrent.clone());
    assert_eq!(
        fixture
            .registry
            .compare_replace(
                RegistrySlot::UserPath,
                &previous,
                Some(&RegistryState::default())
            )
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    assert_eq!(
        fixture.registry.read(RegistrySlot::UserPath).unwrap(),
        concurrent
    );
}
