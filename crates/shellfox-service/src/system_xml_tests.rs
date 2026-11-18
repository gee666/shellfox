use super::super::*;
use super::support::Fixture;
use shellfox_protocol::ErrorCode;
use std::fs;

#[test]
fn linux_menu_installation_preserves_foreign_thunar_actions_and_is_read_only_until_requested() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let xml = fixture.root.join("config/Thunar/uca.xml");
    fs::create_dir_all(xml.parent().unwrap()).unwrap();
    let original="<?xml version='1.0'?><actions><!-- preserve --><action><unique-id>foreign</unique-id><command>foreign --arg</command></action></actions><!-- literal </actions> -->";
    fs::write(&xml, original).unwrap();
    let services = HostSystemServices::new(fixture.backend(
        vec![FileManager::Thunar],
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
    assert_eq!(fs::read_to_string(&xml).unwrap(), original);
    assert!(!fixture.plan.directory.exists());
    services
        .perform_user_action(SystemAction::SetExplorer {
            installed: true,
            scope: SystemScope::User,
        })
        .unwrap();
    let installed = fs::read_to_string(&xml).unwrap();
    assert!(installed.contains(
        "<action><unique-id>foreign</unique-id><command>foreign --arg</command></action>"
    ));
    assert!(installed.ends_with("<!-- literal </actions> -->"));
    assert!(
        services
            .explorer_integration(SystemScope::User)
            .unwrap()
            .installed
    );
    services
        .perform_user_action(SystemAction::SetExplorer {
            installed: false,
            scope: SystemScope::User,
        })
        .unwrap();
    assert_eq!(fs::read_to_string(xml).unwrap(), original);
}
#[test]
fn xml_rejects_dtd_entities_node_bombs_and_foreign_owned_id() {
    for bad in [
        "<!DOCTYPE actions [<!ENTITY x SYSTEM 'file:///not-read'>]><actions>&x;</actions>",
        "<actions><!ENTITY x 'bad'></actions>",
        "<wrong/>",
    ] {
        assert!(thunar::merge(Some(bad), "trusted start %f", true).is_err());
    }
    let many = format!("<actions>{}</actions>", "<a/>".repeat(20001));
    assert!(thunar::merge(Some(&many), "trusted start %f", true).is_err());
    let foreign="<actions><action><unique-id>shellfox-linux-v1</unique-id><command>foreign</command></action></actions>";
    assert_eq!(
        thunar::merge(Some(foreign), "trusted start %f", true)
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
}
#[test]
fn temporary_acceptance_paths_cannot_become_production_install_resources() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    assert!(ConfiguredSystemResources::from_validated_package(
        fixture.root.join("package"),
        fixture.plan.clone(),
        None,
        None,
        std::sync::Arc::new(())
    )
    .is_err());
}
