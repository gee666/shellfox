use super::*;
fn identity() -> Identity {
    Identity {
        session_id: shellfox_core::SessionId::new(),
        tab_id: TabId::new(),
        generation: shellfox_core::Generation::new(),
    }
}
#[test]
fn captured_route_never_comes_from_later_config_and_excludes_guest_and_program() {
    let data = crate::tests::scratch();
    let mut config = PtyConfig::new(data.path());
    config.profile = Some(ShellProfileId::Cmd);
    let captured = capture_catalog_origin(&config);
    config.profile = Some(ShellProfileId::Wsl);
    assert_eq!(captured.profile, Some(ShellProfileId::Cmd));
    assert!(capture_catalog_origin(&config).profile.is_none());
    config.profile = Some(ShellProfileId::Cmd);
    config.command = Some(shellfox_terminal::PtyCommand {
        program: "owned-fixture".into(),
        args: vec![],
    });
    assert!(capture_catalog_origin(&config).profile.is_none());
    config.command = None;
    config.profile = None;
    assert!(capture_catalog_origin(&config).profile.is_none());
}
#[test]
fn policy_record_is_neither_ready_inference_nor_a_grant_for_uncaptured_sources() {
    let id = identity();
    let enabled = FactoryPolicyState {
        policy: NativeFactoryPolicy::ExistingManagerStandardCatalog,
        revision: 3,
    };
    let mut local = SourceShareRecord::local_only(id);
    local.activate(id, true, &enabled);
    assert!(local.eligible_revision.is_none());
    let mut catalog = SourceShareRecord::captured(
        id,
        CapturedCatalogOrigin {
            profile: Some(ShellProfileId::Cmd),
        },
    );
    catalog.activate(id, false, &enabled);
    assert!(catalog.eligible_revision.is_none());
    catalog.activate(identity(), true, &enabled);
    assert!(catalog.eligible_revision.is_none());
    catalog.activate(id, true, &FactoryPolicyState::default());
    assert!(catalog.eligible_revision.is_none());
    catalog.activate(id, true, &enabled);
    assert_eq!(catalog.eligible_revision, Some(3));
    // This is policy-only state. No privilege, peer permission or allow result is constructed.
}
