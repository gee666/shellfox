use shellfox_protocol::Validate;
use shellfox_remote::adapter::persisted_settings;
#[test]
fn full_persisted_settings_preserve_every_field_and_nullable_profile_state() {
    for profile in [
        None,
        Some(None),
        Some(Some("opaque-imported-profile".into())),
    ] {
        let mut settings = shellfox_core::Settings {
            history_page_size: 17,
            session_settle_hours: 48.0,
            session_delete_hours: 1440.0,
            auto_update: false,
            terminal_profile_id: profile,
            terminal_env: vec![shellfox_core::EnvVar {
                name: "SHARED".into(),
                value: "exact".into(),
            }],
            ..Default::default()
        };
        let dto = persisted_settings(&settings).unwrap();
        dto.validate().unwrap();
        let mut wire = serde_json::to_value(&dto).unwrap();
        let legacy_python = wire.as_object_mut().unwrap().remove("pythonPath");
        assert_eq!(legacy_python, Some(serde_json::Value::Null));
        assert_eq!(wire, serde_json::to_value(&settings).unwrap());
        settings.history_page_size = 0;
        assert!(persisted_settings(&settings).is_err());
    }
}
