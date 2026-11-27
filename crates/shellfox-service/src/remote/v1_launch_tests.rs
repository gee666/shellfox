use super::*;
use shellfox_core::CreateSession;

#[test]
fn a_peer_activating_a_session_without_tabs_opens_one_in_the_folder_profile() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let profile = crate::tests::default_catalog_profile();
    let created = service
        .host()
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: profile.clone(),
        })
        .unwrap();
    let (session, tab) = (created.sessions[0].id, created.sessions[0].tabs[0].id);
    service.host().close_tab(tab).unwrap();
    let tabs = |service: &Service| service.host().snapshot().unwrap().sessions[0].tabs.clone();
    assert!(tabs(&service).is_empty(), "the session stays without tabs");
    let mut state = RemoteState::new(
        Default::default(),
        Some(wire::TerminalProfiles {
            profiles: vec![wire::TerminalProfile {
                id: profile.as_str().into(),
                label: "Fixture shell".into(),
                environment: wire::ProfileEnvironment::Local,
                executable: std::env::current_exe().unwrap().to_string_lossy().into(),
                args: Vec::new(),
                distro: None,
                available: true,
                unavailable_reason: None,
                can_terminate_descendants: Some(true),
                elevation: None,
            }],
            default_profile_id: Some(profile.as_str().into()),
            lifetime: wire::TerminalLifetime::AppOwned,
            shell_survival: false,
        }),
        None,
    );
    // The test binary is no PTY supervisor, so the new shell fails to start;
    // its tab is made regardless, as on the local route.
    let _ = state
        .bind(&mut service)
        .activate_session(wire::ActivateSession {
            session_id: session,
        });
    let reopened = tabs(&service);
    assert_eq!(reopened.len(), 1);
    assert_eq!(reopened[0].profile_id, profile);
    service.shutdown().unwrap();
}
