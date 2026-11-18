use super::*;
use shellfox_core::{PreferencesPatch, ProfileId, SessionId};

#[test]
fn application_creation_preserves_selected_catalog_intent_not_current_settings() {
    let data = crate::tests::scratch();
    let service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let input = |profile: &str| CreateSession {
        request_id: SessionId::new(),
        title: None,
        cwd: data.path().to_string_lossy().into_owned(),
        profile_id: ProfileId::new(profile).unwrap(),
    };
    let created = service.create_application_session(input("cmd")).unwrap();
    let id = created.selection.session_id.unwrap();
    let actual = service.host.session_creation_facts().unwrap();
    assert_eq!(actual[&id].adapter_id, SettingsAdapter::EmbeddedPty);
    assert_eq!(actual[&id].shell_id, SettingsShell::LoginShell);
    service
        .host
        .update_preferences(PreferencesPatch {
            default_profile_id: Some(ProfileId::new("pwsh").unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(service.host.session_creation_facts().unwrap(), actual);
    let pure = service
        .host
        .create_session(input("opaque:unknown"))
        .unwrap()
        .selection
        .session_id
        .unwrap();
    assert!(!service
        .host
        .session_creation_facts()
        .unwrap()
        .contains_key(&pure));
    let revision = service.host.snapshot().unwrap().revision;
    assert!(service
        .create_application_session(input("opaque:unknown"))
        .is_err());
    assert_eq!(service.host.snapshot().unwrap().revision, revision);
}
