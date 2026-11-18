use super::*;
use shellfox_core::{CreateSession, ProfileId, SessionId};
use shellfox_remote::adapter::ManagerFacts;
fn fixture() -> (tempfile::TempDir, Service, RemoteState) {
    let data = crate::tests::scratch();
    let service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    (
        data,
        service,
        RemoteState::new(ManagerFacts::default(), None, None),
    )
}
fn create(service: &Service, cwd: &std::path::Path, profile: &str) -> SessionId {
    let id = SessionId::new();
    service
        .host()
        .create_session(CreateSession {
            request_id: id,
            title: Some("saved metadata".into()),
            cwd: cwd.to_string_lossy().into_owned(),
            profile_id: ProfileId::new(profile).unwrap(),
        })
        .unwrap();
    let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } =
        service.host().creation_request_receipt(id).unwrap()
    else {
        panic!("creation receipt")
    };
    session_id
}
#[test]
fn policy_eligibility_without_actual_native_normal_evidence_is_never_effective_delivery() {
    let (data, mut service, _state) = fixture();
    let identity = shellfox_runtime::Identity { session_id: SessionId::new(), tab_id: shellfox_core::TabId::new(), generation: shellfox_core::Generation::new() };
    let runtime = shellfox_runtime::Runtime::start_external(identity, Default::default()).unwrap();
    let mut terminal = crate::terminals::OwnedTerminal::new(runtime);
    // Policy-only fixture, not a native allow/token fixture. Even a captured
    // candidate and genuine parser Started cannot turn absent launch proof Normal.
    let mut config = shellfox_terminal::PtyConfig::new(data.path()); config.profile = Some(shellfox_terminal::ShellProfileId::Cmd);
    terminal.source_share = crate::native_sharing::SourceShareRecord::captured(identity, crate::native_sharing::capture_catalog_origin(&config));
    service.terminals.insert(identity.tab_id, terminal);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while service.terminals[&identity.tab_id].startup.is_none() {
        assert!(std::time::Instant::now() < deadline); service.drain(identity.tab_id).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    service.install_observer_factory_policy(crate::native_sharing::NativeFactoryPolicy::ExistingManagerStandardCatalog).unwrap();
    assert!(service.observer_delivery_policy(identity).is_some());
    assert_eq!(service.terminals[&identity.tab_id].runtime.launch_privilege_facts().privilege, shellfox_runtime::LaunchPrivilege::Unknown);
    assert!(runtime::effective_policy(&service, identity).is_none());
}
#[test]
fn complete_mixed_metadata_never_needs_legacy_creation_or_current_profile_defaults() {
    let (data, mut service, state) = fixture();
    let raw = create(&service, data.path(), "automation-pipe");
    let unknown = create(&service, data.path(), "not-in-current-catalog");
    let normal = SessionId::new();
    service
        .host()
        .create_session_with_facts(
            CreateSession {
                request_id: normal,
                title: Some("normal".into()),
                cwd: data.path().to_string_lossy().into_owned(),
                profile_id: ProfileId::new("cmd").unwrap(),
            },
            shellfox_host::SessionCreationFacts::new(
                shellfox_core::SettingsAdapter::EmbeddedPty,
                shellfox_core::SettingsShell::LoginShell,
            ),
        )
        .unwrap();
    let shellfox_host::CreationRequestReceipt::SessionCreated { session_id: normal } =
        service.host().creation_request_receipt(normal).unwrap()
    else {
        panic!("creation receipt")
    };
    let saved = target(&state, &service, raw).unwrap();
    let tab = saved.tabs[0].id;
    let identity = shellfox_runtime::Identity {
        session_id: raw,
        tab_id: tab,
        generation: shellfox_core::Generation::new(),
    };
    let runtime = shellfox_runtime::Runtime::start_external(identity, Default::default()).unwrap();
    service
        .terminals
        .insert(tab, crate::terminals::OwnedTerminal::new(runtime));
    let snapshot = catalog::snapshot(&state, &service).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.coverage.entry_count, 3);
    for id in [raw, unknown, normal] {
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.id().to_string() == id.to_string()));
    }
    let v2::CatalogEntry::Session { value } = snapshot
        .entries
        .iter()
        .find(|entry| entry.id().to_string() == raw.to_string())
        .unwrap()
    else {
        panic!("session")
    };
    assert!(matches!(value.facts.adapter_id, v2::Fact::Unknown { .. }));
    assert!(matches!(value.facts.shell_id, v2::Fact::Unknown { .. }));
    assert!(matches!(
        value.tabs[0].source_kind,
        v2::Fact::Known {
            value: v2::SourceKind::ExternalBytes
        }
    ));
    assert!(matches!(
        value.tabs[0].facts.lifecycle,
        v2::Fact::Unknown { .. }
    ));
    assert!(matches!(
        value.tabs[0].facts.agents,
        v2::Fact::Unknown { .. }
    ));
    assert!(!service
        .host()
        .session_creation_facts()
        .unwrap()
        .contains_key(&raw));
}
#[test]
fn one_unprojectable_authorized_entry_does_not_poison_independent_normal_projection_or_mutation() {
    let (data, service, state) = fixture();
    let first = create(&service, data.path(), "unknown-program");
    let second = create(&service, data.path(), "cmd");
    let mut malformed = target(&state, &service, first).unwrap();
    malformed.title = String::new();
    assert!(
        matches!(catalog::entry(&state,&service,&malformed).unwrap(),v2::CatalogEntry::Unavailable{session_id,..} if session_id.to_string()==first.to_string())
    );
    service
        .host()
        .rename_session(second, "normal still works")
        .unwrap();
    let normal =
        catalog::entry(&state, &service, &target(&state, &service, second).unwrap()).unwrap();
    let v2::CatalogEntry::Session { value } = normal else {
        panic!("normal")
    };
    assert_eq!(value.title, "normal still works");
}
#[test]
fn explicit_local_only_visibility_precedes_diagnostics_but_unknown_saved_facts_are_not_hidden() {
    let (data, service, mut state) = fixture();
    let private = create(&service, data.path(), "admin-only");
    let unknown = create(&service, data.path(), "unknown-program");
    state.profiles = Some(wire::TerminalProfiles {
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
    });
    let private_tab = target(&state, &service, private).unwrap().tabs[0].id;
    let snapshot = catalog::snapshot(&state, &service).unwrap();
    assert_eq!(snapshot.coverage.entry_count, 2);
    assert!(snapshot.entries.iter().any(|entry| entry.id().to_string() == unknown.to_string()));
    let v2::CatalogEntry::Session { value } = snapshot.entries.iter().find(|entry| entry.id().to_string() == private.to_string()).unwrap() else { panic!("saved session") };
    assert!(value.tabs.is_empty());
    let counts = value.facts.counts.known().unwrap(); assert_eq!((counts.running, counts.waiting, counts.unknown, counts.error, counts.closed, counts.agents), (0,0,0,0,0,0));
    assert!(tab_target(&state, &service, private_tab).is_err());
    assert!(authorize_session_sources(&state, &service, &target(&state, &service, private).unwrap()).is_err());
    service.host().rename_session(private, "metadata rename is still authorized").unwrap();
    let encoded = serde_json::to_string(&catalog::snapshot(&state, &service).unwrap()).unwrap();
    assert!(!encoded.contains(&private_tab.to_string()));
    service.host().add_tab(private, shellfox_core::AddTab { title: Some("normal tab".into()), cwd: None, profile_id: ProfileId::new("unknown-standard-metadata").unwrap() }).unwrap();
    let mut bad = service
        .host()
        .snapshot()
        .unwrap()
        .sessions
        .into_iter()
        .find(|s| s.id == private)
        .unwrap();
    bad.tabs[0].title = String::new();
    let projected = catalog::entry(&state, &service, &bad).unwrap();
    let v2::CatalogEntry::Session { value } = projected else { panic!("private tab diagnostics were evaluated") };
    assert_eq!(value.tabs.len(), 1); assert_ne!(value.tabs[0].id.to_string(), private_tab.to_string());
    bad.title = String::new();
    let unavailable = catalog::entry(&state, &service, &bad).unwrap();
    assert!(matches!(unavailable, v2::CatalogEntry::Unavailable { .. }), "authorized session header errors remain represented");
    assert!(!serde_json::to_string(&unavailable).unwrap().contains(&private_tab.to_string()));
}
