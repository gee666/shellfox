use serde_json::json;
use shellfox_core::*;

#[test]
fn uuid_and_profile_deserialization_validate_the_portable_boundary() {
    let id = SessionId::new();
    assert_eq!(
        serde_json::from_value::<SessionId>(json!(id.to_string())).unwrap(),
        id
    );
    for invalid in [
        "not-an-id".to_string(),
        id.to_string().replace('-', ""),
        format!("{{{id}}}"),
        format!("urn:uuid:{id}"),
    ] {
        assert!(invalid.parse::<SessionId>().is_err());
        assert!(serde_json::from_value::<SessionId>(json!(invalid)).is_err());
    }
    for profile in [
        "pwsh",
        "windows-powershell",
        "cmd",
        "wsl",
        "login-shell",
        "bash",
        "wsl:Ubuntu",
        "wsl:Opaque distro (dev)",
        "shell:/bin/zsh",
    ] {
        assert_eq!(
            serde_json::from_value::<ProfileId>(json!(profile))
                .unwrap()
                .as_str(),
            profile
        );
    }
    for profile in ["", "bad\nprofile", "bad\0profile", &"x".repeat(201)] {
        assert!(serde_json::from_value::<ProfileId>(json!(profile)).is_err());
    }
}

#[test]
fn paths_follow_shellfox_local_and_wsl_contracts_on_every_client_os() {
    for path in [
        "/",
        "/home/me/project",
        "C:\\work",
        "d:/work",
        "\\\\wsl$\\Ubuntu",
        "\\\\WSL.localhost\\Ubuntu\\home\\me",
    ] {
        validation::cwd(path).unwrap();
    }
    for path in [
        "",
        "relative",
        "C:relative",
        "//remote/share",
        "\\\\server\\share",
        "\\\\wsl$\\",
        "\\\\wsl$\\Ubuntu/home",
        "file:///tmp",
        "/tmp\nsecret",
        "/\0tmp",
    ] {
        assert!(validation::cwd(path).is_err(), "accepted {path:?}");
    }
}

#[test]
fn preferences_validate_sizes_colors_and_unknown_fields() {
    let defaults = Preferences::default();
    assert_eq!(defaults.accent_color, "#ec4899");
    assert_eq!(defaults.background_color, "#111016");
    defaults.validate().unwrap();
    for size in [f32::NAN, f32::INFINITY, 0.0, 7.9, 40.1] {
        assert!(defaults
            .patched(PreferencesPatch {
                font_size: Some(size),
                ..Default::default()
            })
            .is_err());
    }
    assert!(defaults
        .patched(PreferencesPatch {
            font_family: Some("\nfont".into()),
            ..Default::default()
        })
        .is_err());
    assert!(defaults
        .patched(PreferencesPatch {
            accent_color: Some("#12fg56".into()),
            ..Default::default()
        })
        .is_err());
    assert!(
        serde_json::from_value::<PreferencesPatch>(json!({"shellExecutable":"/bin/sh"})).is_err()
    );
}

#[test]
fn utc_and_environment_validation_have_explicit_case_policy() {
    validation::timestamp("2026-01-01T00:00:00.000Z").unwrap();
    assert!(validation::timestamp("2026-01-01T01:00:00+01:00").is_err());
    assert!(validation::timestamp("today").is_err());
    let env = vec![
        EnvVar {
            name: "PATH".into(),
            value: "/bin".into(),
        },
        EnvVar {
            name: "Path".into(),
            value: "/usr/bin".into(),
        },
    ];
    validate_env(&env, false).unwrap();
    assert!(validate_env(&env, true).is_err());
    assert!(validate_env(
        &[EnvVar {
            name: " A".into(),
            value: "value".into()
        }],
        false
    )
    .is_err());
}
