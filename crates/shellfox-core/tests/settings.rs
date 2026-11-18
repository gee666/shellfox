use serde_json::{json, Value};
use shellfox_core::*;
fn patch(value: Value) -> SettingsPatch {
    serde_json::from_value(value).unwrap()
}

#[test]
fn defaults_match_current_electron_settings_and_all_bundled_rules() {
    let settings = Settings::default();
    settings.validate().unwrap();
    assert_eq!(settings.version, 1);
    assert_eq!(settings.python_path, None);
    assert_eq!(settings.shell_executable, None);
    assert_eq!(settings.terminal_profile_id, None);
    assert!(settings.auto_update);
    assert_eq!(settings.accent_color, "#ec4899");
    assert_eq!(settings.background_color, "#111016");
    assert_eq!(settings.history_page_size, 10);
    assert_eq!(settings.session_settle_hours, 24.0);
    assert_eq!(settings.session_delete_hours, 720.0);
    assert!(settings.terminal_env.is_empty());
    assert_eq!(
        settings.adapter_id,
        if cfg!(target_os = "linux") {
            SettingsAdapter::GnomeTerminal
        } else {
            SettingsAdapter::WindowsTerminal
        }
    );
    assert_eq!(
        settings.shell_id,
        if cfg!(target_os = "linux") {
            SettingsShell::Bash
        } else {
            SettingsShell::Pwsh
        }
    );
    let expected = json!([
        {"id":"f919fb1a-fb03-4a93-8b9b-1cde465d5870","label":"Pi","enabled":true,"processNames":["pi","pi.exe"],"executableBasenames":["node.exe","node","nodejs","bun.exe","bun"],"executablePaths":[],"scriptPathSuffixes":["@earendil-works/pi-coding-agent/dist/cli.js","@mariozechner/pi-coding-agent/dist/cli.js","@earendil-works/pi-coding-agent/dist/bundle/cli.js","@mariozechner/pi-coding-agent/dist/bundle/cli.js"]},
        {"id":"f919fb1a-fb03-4a93-8b9b-1cde465d5871","label":"Claude","enabled":true,"processNames":["claude","claude.exe"],"executableBasenames":["node.exe","node","nodejs","bun.exe","bun"],"executablePaths":[],"scriptPathSuffixes":["@anthropic-ai/claude-code/cli.js"]},
        {"id":"f919fb1a-fb03-4a93-8b9b-1cde465d5872","label":"Codex","enabled":true,"processNames":["codex","codex.exe"],"executableBasenames":["node.exe","node","nodejs","bun.exe","bun"],"executablePaths":[],"scriptPathSuffixes":["@openai/codex/bin/codex.js"]},
        {"id":"f919fb1a-fb03-4a93-8b9b-1cde465d5874","label":"OpenCode","enabled":true,"processNames":["opencode","opencode.exe"],"executableBasenames":["node.exe","node","nodejs","bun.exe","bun"],"executablePaths":[],"scriptPathSuffixes":["opencode-ai/bin/opencode"]}
    ]);
    assert_eq!(
        serde_json::to_value(settings.process_rules).unwrap(),
        expected
    );
}
#[test]
fn patch_preserves_omissions_and_distinguishes_nullable_clears() {
    let settings = Settings::default().patched(patch(json!({"pythonPath":"/usr/bin/python3","shellExecutable":"/bin/bash","terminalProfileId":"wsl:An opaque distro (dev)"}))).unwrap();
    assert_eq!(settings.patched(patch(json!({}))).unwrap(), settings);
    let cleared = settings
        .patched(patch(
            json!({"pythonPath":null,"shellExecutable":null,"terminalProfileId":null}),
        ))
        .unwrap();
    assert_eq!(cleared.python_path, None);
    assert_eq!(cleared.shell_executable, None);
    assert_eq!(cleared.terminal_profile_id, Some(None));
    for field in [
        "version",
        "accentColor",
        "backgroundColor",
        "adapterId",
        "shellId",
        "processRules",
        "historyPageSize",
        "sessionSettleHours",
        "sessionDeleteHours",
        "autoUpdate",
        "terminalEnv",
    ] {
        assert!(
            serde_json::from_value::<SettingsPatch>(json!({field:null})).is_err(),
            "accepted null {field}"
        );
    }
    assert!(serde_json::from_value::<SettingsPatch>(json!({"fontSize":16})).is_err());
    assert!(serde_json::from_value::<SettingsPatch>(json!({"sidebarWidth":300})).is_err());
}
#[test]
fn complete_dto_read_defaults_are_not_patch_write_defaults() {
    let mut value = serde_json::to_value(Settings::default()).unwrap();
    for field in [
        "pythonPath",
        "backgroundColor",
        "historyPageSize",
        "sessionSettleHours",
        "sessionDeleteHours",
        "autoUpdate",
        "terminalEnv",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    let settings: Settings = serde_json::from_value(value.clone()).unwrap();
    settings.validate().unwrap();
    assert!(settings.auto_update);
    assert_eq!(settings.terminal_env, vec![]);
    value.as_object_mut().unwrap().remove("shellExecutable");
    assert!(serde_json::from_value::<Settings>(value).is_err());
}
#[test]
fn validation_rejects_invalid_settings_without_command_execution_fields() {
    let settings = Settings::default();
    for value in [
        json!({"version":2}),
        json!({"accentColor":"red"}),
        json!({"backgroundColor":"#00000g"}),
        json!({"historyPageSize":0}),
        json!({"historyPageSize":101}),
        json!({"sessionSettleHours":0.5}),
        json!({"sessionDeleteHours":87601}),
        json!({"pythonPath":"relative"}),
        json!({"terminalProfileId":""}),
        json!({"shellExecutable":"https://bad.test/program"}),
        json!({"terminalEnv":[{"name":"A","value":"a\nb"}]}),
    ] {
        assert!(
            settings.patched(patch(value.clone())).is_err(),
            "accepted {value}"
        );
    }
    assert!(serde_json::from_value::<SettingsPatch>(json!({"command":"touch unsafe"})).is_err());
    let mut invalid = settings.clone();
    invalid.session_settle_hours = f64::NAN;
    assert!(invalid.validate().is_err());
    invalid.session_settle_hours = f64::INFINITY;
    assert!(invalid.validate().is_err());
}
#[test]
fn js_integer_numbers_accept_integral_json_floats_but_not_fractional_values() {
    let parsed: SettingsPatch =
        serde_json::from_str(r#"{"version":1.0,"historyPageSize":17.0}"#).unwrap();
    let settings = Settings::default().patched(parsed).unwrap();
    assert_eq!(settings.history_page_size, 17);
    for raw in [
        r#"{"version":1.5}"#,
        r#"{"historyPageSize":17.5}"#,
        r#"{"historyPageSize":-1}"#,
    ] {
        assert!(serde_json::from_str::<SettingsPatch>(raw).is_err());
    }
}
#[test]
fn process_rule_validation_matches_actual_constraints() {
    let base = default_process_rules().remove(0);
    let mut whitespace = base.clone();
    whitespace.label = "\u{feff}".into();
    assert!(whitespace.validate().is_err());
    whitespace.label = "\u{0085}".into();
    whitespace.validate().unwrap();
    let mut raw = serde_json::to_value(&base).unwrap();
    raw["processNames"] = Value::Null;
    assert!(serde_json::from_value::<SettingsProcessRule>(raw).is_err());
    for id in [
        "custom-a-1",
        "custom-my-tool-2025",
        "00000000-0000-0000-0000-000000000000",
    ] {
        let mut rule = base.clone();
        rule.id = id.into();
        rule.validate().unwrap();
    }
    for id in [
        "custom-X-1",
        "custom-a-",
        "arbitrary",
        "f919fb1a-fb03-fa93-8b9b-1cde465d5870",
    ] {
        let mut rule = base.clone();
        rule.id = id.into();
        assert!(rule.validate().is_err());
    }
    for suffix in ["../cli.js", "x/./y", "x\\..\\y", "\n"] {
        let mut rule = base.clone();
        rule.script_path_suffixes = vec![suffix.into()];
        assert!(rule.validate().is_err());
    }
    for name in ["/bin/program", "bad:name", "bad\\name", "  "] {
        let mut rule = base.clone();
        rule.executable_basenames = vec![name.into()];
        assert!(rule.validate().is_err());
    }
    let mut settings = Settings::default();
    settings.process_rules.push(base);
    assert!(settings.validate().is_err());
    let mut rule = default_process_rules().remove(0);
    rule.executable_basenames.clear();
    rule.process_names = Some(vec!["pi".into()]);
    assert!(rule.validate().is_err());
}
#[test]
fn environment_normalizes_names_and_counts_utf16_not_utf8_bytes() {
    let name = "界".repeat(256);
    let settings = Settings::default()
        .patched(patch(
            json!({"terminalEnv":[{"name":format!(" {name} "),"value":"user value"}]}),
        ))
        .unwrap();
    assert_eq!(settings.terminal_env[0].name, name);
    let normalized = normalize_env(
        vec![EnvVar {
            name: "\u{feff}\u{00a0} A \u{feff}".into(),
            value: "v".into(),
        }],
        false,
    )
    .unwrap();
    assert_eq!(normalized[0].name, "A");
    // ECMAScript accepts U+0085 in a name; Rust's is_whitespace would reject it.
    normalize_env(
        vec![EnvVar {
            name: "A\u{0085}B".into(),
            value: "v".into(),
        }],
        false,
    )
    .unwrap();
    for name in ["A\u{feff}B", "A\u{2028}B"] {
        assert!(normalize_env(
            vec![EnvVar {
                name: name.into(),
                value: "v".into()
            }],
            false
        )
        .is_err());
    }
    assert!(Settings::default()
        .patched(patch(
            json!({"terminalEnv":[{"name":" A ","value":"1"},{"name":"A","value":"2"}]})
        ))
        .is_err());
}
#[test]
fn bundled_upgrade_preserves_toggles_custom_paths_and_unique_ids() {
    let mut settings = Settings::default();
    settings.process_rules[0].enabled = false;
    settings.process_rules[0].process_names = None;
    settings.process_rules[0].script_path_suffixes.truncate(2);
    settings.process_rules[1].executable_paths = vec!["/opt/my-claude".into()];
    let before = settings.process_rules[1].clone();
    settings.upgrade_bundled_rules();
    settings.validate().unwrap();
    assert!(!settings.process_rules[0].enabled);
    assert_eq!(settings.process_rules[0].script_path_suffixes.len(), 4);
    assert_eq!(
        settings.process_rules[1].executable_paths,
        before.executable_paths
    );
    assert_eq!(settings.process_rules[1].id, "custom-bundled-claude-legacy");
    let upgraded = settings.clone();
    settings.upgrade_bundled_rules();
    assert_eq!(settings, upgraded);
}
