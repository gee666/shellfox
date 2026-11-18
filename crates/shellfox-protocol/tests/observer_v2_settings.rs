use serde_json::{json, Value};
use shellfox_protocol::{
    observer_v2::{self as v2, Fact, Settings},
    Validate,
};
#[path = "support/json_presence.rs"]
mod json_presence;
fn minimal() -> Value {
    json!({"version":1,"accentColor":"#ec4899","adapterId":"embedded-pty","shellId":"pwsh","shellExecutable":null,"processRules":[]})
}
fn roundtrip(raw: &Value) -> Settings {
    let settings: Settings = serde_json::from_value(raw.clone()).unwrap();
    settings.validate().unwrap();
    json_presence::assert_same(&serde_json::to_value(&settings).unwrap(), raw);
    settings
}
#[test]
fn v2_does_not_insert_python_time_color_environment_or_boolean_defaults() {
    let raw = minimal();
    let settings = roundtrip(&raw);
    assert_eq!(settings.python_path, None);
    assert_eq!(settings.terminal_profile_id, None);
    assert_eq!(settings.session_settle_hours, None);
    assert_eq!(settings.session_delete_hours, None);
    assert_eq!(settings.history_page_size, None);
    assert_eq!(settings.auto_update, None);
    assert!(settings.background_color.is_none() && settings.terminal_env.is_none());
    let legacy: shellfox_protocol::SettingsDto = serde_json::from_value(raw).unwrap();
    legacy.validate().unwrap();
    let legacy = serde_json::to_value(legacy).unwrap();
    assert!(legacy.get("pythonPath").unwrap().is_null());
    assert_eq!(legacy["sessionSettleHours"], json!(24.0));
    assert_eq!(legacy["autoUpdate"], true);
    // Existing v1 migration behavior is intentionally not changed.
}
#[test]
fn explicit_nullable_absence_is_preserved_only_when_present() {
    for field in ["pythonPath", "terminalProfileId"] {
        let settings = roundtrip(&minimal());
        assert!(serde_json::to_value(settings).unwrap().get(field).is_none());
        for value in [
            Value::Null,
            if field == "pythonPath" {
                json!("/fixture/python")
            } else {
                json!("pwsh")
            },
        ] {
            let mut raw = minimal();
            raw[field] = value.clone();
            let settings = roundtrip(&raw);
            let encoded = serde_json::to_value(settings).unwrap();
            assert_eq!(encoded.get(field), Some(&value));
        }
    }
}
#[test]
fn every_formerly_defaulted_field_preserves_omission_and_actual_value() {
    let values = [
        ("backgroundColor", json!("#112233")),
        ("historyPageSize", json!(10)),
        ("sessionSettleHours", json!(24)),
        ("sessionDeleteHours", json!(720)),
        ("autoUpdate", json!(false)),
        ("terminalEnv", json!([])),
    ];
    for (field, value) in values {
        let mut raw = minimal();
        raw[field] = value;
        roundtrip(&raw);
        raw[field] = Value::Null;
        assert!(
            serde_json::from_value::<Settings>(raw).is_err(),
            "{field} is optional, not nullable"
        );
    }
    let mut raw = minimal();
    raw["sessionSettleHours"] = json!(24.0);
    roundtrip(&raw);
}
#[test]
fn process_rule_optional_names_are_not_defaulted_or_null_coerced() {
    let rule = json!({"id":"10000000-0000-4000-8000-000000000001","label":"Agent","enabled":true,"executableBasenames":["agent"],"executablePaths":[],"scriptPathSuffixes":[]});
    let mut raw = minimal();
    raw["processRules"] = json!([rule]);
    roundtrip(&raw);
    raw["processRules"][0]["processNames"] = json!([]);
    roundtrip(&raw);
    raw["processRules"][0]["processNames"] = Value::Null;
    assert!(serde_json::from_value::<Settings>(raw).is_err());
}
#[test]
fn settings_fact_and_response_uses_the_presence_aware_leaf_not_v1_alias() {
    let fact = json!({"state":"known","value":minimal()});
    let parsed: Fact<Settings> = serde_json::from_value(fact.clone()).unwrap();
    parsed.validate().unwrap();
    json_presence::assert_same(&serde_json::to_value(parsed).unwrap(), &fact);
    let all: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/observer_v2/methods.json")).unwrap();
    let original = all
        .into_iter()
        .find(|v| v["method"] == "setTerminalEnv")
        .unwrap()["response"]
        .clone();
    assert!(
        original["value"].get("pythonPath").is_none(),
        "Do not change the canonical fixture to mask default insertion"
    );
    let reply: v2::BoundReply<<v2::SetTerminalEnv as v2::Request>::Response> =
        shellfox_protocol::decode(&serde_json::to_vec(&original).unwrap()).unwrap();
    json_presence::assert_same(&serde_json::to_value(reply).unwrap(), &original);
}
#[test]
fn reused_probe_optionals_preserve_omission_and_reject_null_instead_of_coercing() {
    let baseline: Value = serde_json::from_str(include_str!(
        "../../shellfox-client/tests/fixtures/manager-snapshot-success.json"
    ))
    .unwrap();
    let mut raw: Value =
        serde_json::from_str(include_str!("fixtures/observer_v2/snapshot.json")).unwrap();
    let mut probe = baseline["value"]["probe"].clone();
    probe.as_object_mut().unwrap().remove("python");
    for field in ["embeddedTerminal", "terminalLifetime", "shellSurvival"] {
        probe["capabilities"].as_object_mut().unwrap().remove(field);
    }
    raw["value"]["probe"] = json!({"state":"known","value":probe});
    let parsed: v2::BoundReply<v2::Snapshot> =
        shellfox_protocol::decode(&serde_json::to_vec(&raw).unwrap()).unwrap();
    json_presence::assert_same(&serde_json::to_value(parsed).unwrap(), &raw);
    raw["value"]["probe"]["value"]["capabilities"]["embeddedTerminal"] = Value::Null;
    assert!(shellfox_protocol::decode::<v2::BoundReply<v2::Snapshot>>(
        &serde_json::to_vec(&raw).unwrap()
    )
    .is_err());
}
#[test]
fn reused_terminal_profile_optionals_preserve_absent_null_and_false_exactly() {
    let base = json!({"profiles":[{"id":"pwsh","label":"PowerShell","environment":"local","executable":"C:\\fixture\\pwsh.exe","args":[],"distro":null,"available":true}],"defaultProfileId":null,"lifetime":"app-owned","shellSurvival":false});
    for with_fields in [false, true] {
        let mut raw = base.clone();
        if with_fields {
            raw["profiles"][0]["unavailableReason"] = Value::Null;
            raw["profiles"][0]["canTerminateDescendants"] = json!(false);
        }
        let parsed: shellfox_protocol::TerminalProfiles =
            serde_json::from_value(raw.clone()).unwrap();
        parsed.validate().unwrap();
        json_presence::assert_same(&serde_json::to_value(parsed).unwrap(), &raw);
    }
    let mut invalid = base;
    invalid["profiles"][0]["canTerminateDescendants"] = Value::Null;
    assert!(serde_json::from_value::<shellfox_protocol::TerminalProfiles>(invalid).is_err());
}
#[test]
fn known_null_is_not_synthesized_by_other_fact_or_time_fields() {
    let raw = json!({"state":"known","value":null});
    let parsed: Fact<Option<String>> = serde_json::from_value(raw.clone()).unwrap();
    json_presence::assert_same(&serde_json::to_value(parsed).unwrap(), &raw);
    assert!(serde_json::from_value::<Fact<Option<String>>>(json!({"state":"known"})).is_err());
    let original: Value =
        serde_json::from_str(include_str!("fixtures/observer_v2/snapshot.json")).unwrap();
    let reply: v2::BoundReply<v2::Snapshot> =
        shellfox_protocol::decode(&serde_json::to_vec(&original).unwrap()).unwrap();
    json_presence::assert_same(&serde_json::to_value(reply).unwrap(), &original);
}
