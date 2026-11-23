use super::*;
use shellfox_core::{CreateSession, ProfileId};
#[test]
fn legacy_all_elevated_session_metadata_is_authorized_but_private_tab_facts_are_never_required_or_returned(
) {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let request = SessionId::new();
    service
        .host()
        .create_session_with_facts(
            CreateSession {
                request_id: request,
                title: Some("session metadata".into()),
                cwd: data.path().to_string_lossy().into_owned(),
                profile_id: ProfileId::new("admin-only").unwrap(),
            },
            shellfox_host::SessionCreationFacts::new(
                shellfox_core::SettingsAdapter::EmbeddedPty,
                shellfox_core::SettingsShell::WindowsPowershell,
            ),
        )
        .unwrap();
    let mut snapshot = service.host().snapshot().unwrap();
    let session = snapshot.sessions[0].id;
    let tab = snapshot.sessions[0].tabs[0].id;
    snapshot.sessions[0].tabs[0].created_at = None;
    let private = std::collections::HashSet::from([tab]);
    let source = crate::activity::integration::SourceFacts {
        sequence: 1,
        snapshot,
        settings: service.host().global_settings().unwrap(),
        terminals: Vec::new(),
        creation_facts: service.host().session_creation_facts().unwrap(),
        #[cfg(windows)]
        wsl_profiles: None,
        cancelled: wire::CancellationToken::new(),
    };
    let facts =
        crate::facts::projection::project_session_facts_visible(&source, Some(session), &private)
            .unwrap();
    assert!(facts[&session].tabs.is_empty());
    let mut state = RemoteState::new(
        ManagerFacts {
            sessions: facts,
            ..Default::default()
        },
        Some(wire::TerminalProfiles {
            profiles: vec![wire::TerminalProfile {
                id: "admin-only".into(),
                label: "Administrator".into(),
                environment: wire::ProfileEnvironment::Local,
                executable: "C:\\Windows\\System32\\cmd.exe".into(),
                args: Vec::new(),
                distro: None,
                available: true,
                unavailable_reason: None,
                can_terminate_descendants: Some(true),
                elevation: Some(wire::Elevation::Administrator),
            }],
            default_profile_id: None,
            lifetime: wire::TerminalLifetime::AppOwned,
            shell_survival: false,
        }),
        None,
    );
    let rename = wire::RenameSession {
        session_id: session,
        title: "authorized rename".into(),
    };
    state
        .bind(&mut service)
        .authorize_profiles(&RemoteCommand::RenameSession(rename.clone()))
        .unwrap();
    let response = state
        .bind(&mut service)
        .rename_session_metadata(rename)
        .unwrap();
    assert_eq!(response["tabs"], serde_json::json!([]));
    assert_eq!(
        response["counts"],
        serde_json::json!({"running":0,"waiting":0,"unknown":0,"error":0,"closed":0,"agents":0})
    );
    assert!(!serde_json::to_string(&response)
        .unwrap()
        .contains(&tab.to_string()));
    assert!(state
        .bind(&mut service)
        .authorize_profiles(&RemoteCommand::DeleteSession(wire::DeleteSession {
            session_id: session
        }))
        .is_err());
}
#[test]
fn legacy_normal_target_rename_stats_and_file_privacy_do_not_preflight_unrelated_raw_birth() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let create = |profile: &str, facts: bool| {
        let request = SessionId::new();
        let input = CreateSession {
            request_id: request,
            title: Some(profile.into()),
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new(profile).unwrap(),
        };
        if facts {
            service
                .host()
                .create_session_with_facts(
                    input,
                    shellfox_host::SessionCreationFacts::new(
                        shellfox_core::SettingsAdapter::EmbeddedPty,
                        shellfox_core::SettingsShell::LoginShell,
                    ),
                )
                .unwrap();
        } else {
            service.host().create_session(input).unwrap();
        }
        let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } =
            service.host().creation_request_receipt(request).unwrap()
        else {
            panic!("receipt")
        };
        session_id
    };
    let normal = create("cmd", true);
    let raw = create("automation-pipe", false);
    let source = crate::activity::integration::SourceFacts {
        sequence: 1,
        snapshot: service.host().snapshot().unwrap(),
        settings: service.host().global_settings().unwrap(),
        terminals: Vec::new(),
        creation_facts: service.host().session_creation_facts().unwrap(),
        #[cfg(windows)]
        wsl_profiles: None,
        cancelled: wire::CancellationToken::new(),
    };
    let scoped =
        crate::facts::projection::project_session_facts_for(&source, Some(normal)).unwrap();
    let mut state = RemoteState::new(
        ManagerFacts {
            sessions: scoped,
            ..Default::default()
        },
        Some(wire::TerminalProfiles {
            profiles: vec![wire::TerminalProfile {
                id: "cmd".into(),
                label: "Command Prompt".into(),
                environment: wire::ProfileEnvironment::Local,
                executable: "C:\\Windows\\System32\\cmd.exe".into(),
                args: Vec::new(),
                distro: None,
                available: true,
                unavailable_reason: None,
                can_terminate_descendants: Some(true),
                elevation: None,
            }],
            default_profile_id: Some("cmd".into()),
            lifetime: wire::TerminalLifetime::AppOwned,
            shell_survival: false,
        }),
        None,
    );
    let renamed = state
        .bind(&mut service)
        .rename_session_metadata(wire::RenameSession {
            session_id: normal,
            title: "normal works".into(),
        })
        .unwrap();
    assert_eq!(renamed["title"], "normal works");
    state
        .bind(&mut service)
        .set_session_env_metadata(wire::SetSessionEnv {
            session_id: normal,
            env: vec![shellfox_core::EnvVar {
                name: "SAVED".into(),
                value: "owned".into(),
            }],
        })
        .unwrap();
    let before = service.host().snapshot().unwrap();
    let denied = state
        .bind(&mut service)
        .rename_session_metadata(wire::RenameSession {
            session_id: raw,
            title: "must not mutate".into(),
        })
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::Unsupported);
    assert!(!denied.retryable);
    assert_eq!(service.host().snapshot().unwrap(), before);
    assert_eq!(
        state
            .bind(&mut service)
            .set_session_env_metadata(wire::SetSessionEnv {
                session_id: raw,
                env: Vec::new()
            })
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(service.host().snapshot().unwrap(), before);
    let stats = state
        .bind(&mut service)
        .statistics(wire::GetSessionStats {
            session_id: Some(normal),
        })
        .unwrap();
    let stats: wire::SessionStats = serde_json::from_value(stats).unwrap();
    stats.validate().unwrap();
    assert_eq!(stats.sessions.len(), 1);
    assert_eq!(stats.sessions[0].id, normal);
    // A finished capture fills the live tab's row; an error becomes every row's reason.
    let tab = stats.tabs[0].tab_id;
    let measured = wire::TabStat {
        tab_id: tab,
        processes: Vec::new(),
        memory_bytes: Some(42),
        elapsed_seconds: Some(1.0),
        reason: None,
    };
    state.stats.measured = Some(Ok(vec![measured.clone()]));
    let stats = state
        .bind(&mut service)
        .statistics(wire::GetSessionStats {
            session_id: Some(normal),
        })
        .unwrap();
    assert_eq!(stats["tabs"], serde_json::json!([measured]));
    assert!(
        state.stats.measured.is_none(),
        "a measurement answers one request"
    );
    state.stats.measured = Some(Err(anyhow::anyhow!(
        "Session stats are busy; retry shortly."
    )));
    let stats = state
        .bind(&mut service)
        .statistics(wire::GetSessionStats {
            session_id: Some(normal),
        })
        .unwrap();
    assert_eq!(
        stats["tabs"][0]["reason"],
        "Session stats are busy; retry shortly."
    );
    let read:RemoteCommand=serde_json::from_value(serde_json::json!({"method":"readFile","payload":{"sessionId":normal,"path":data.path().join("owned.txt").to_string_lossy()}})).unwrap();
    state.bind(&mut service).authorize_profiles(&read).unwrap();
    assert!(
        matches!(
            crate::facts::projection::project_session_facts(&source),
            Err(AppError {
                code: ErrorCode::Unsupported,
                retryable: false,
                ..
            })
        ),
        "the strict projection still reports an unsupported row"
    );
    let catalog =
        crate::facts::projection::project_legacy_catalog(&source, None, &Default::default())
            .unwrap();
    state.facts.sessions = catalog.sessions;
    state.facts.omitted_sessions = catalog.omitted_sessions;
    let stats = state
        .bind(&mut service)
        .statistics(wire::GetSessionStats { session_id: None })
        .unwrap();
    let stats: wire::SessionStats = serde_json::from_value(stats).unwrap();
    stats.validate().unwrap();
    assert_eq!(stats.sessions.len(), 1);
    assert_eq!(stats.sessions[0].id, normal);
    assert_eq!(source.snapshot.sessions.len(), 2);
    assert!(!source.creation_facts.contains_key(&raw));
}
