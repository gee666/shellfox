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
#[cfg(feature = "experimental-shellfox-native")]
#[test]
fn native0_creation_requires_explicit_config_and_original_catalog_capture() {
    use shellfox_ghostty_vt::extension::{RestorationConfig, SnapshotLimits};
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let captured = CapturedCatalogOrigin {
        profile: Some(ShellProfileId::LoginShell),
    };
    assert!(service
        .catalog_native0_creation(&captured)
        .unwrap()
        .is_none());
    let configuration = RestorationConfig {
        runtime_image_storage_limit: 10_000_000,
        runtime_resize_pull_scrollback: false,
    };
    let limits = SnapshotLimits::default();
    service
        .install_observer_factory_policy(NativeFactoryPolicy::ExistingManagerNative0Catalog {
            configuration,
            limits,
        })
        .unwrap();
    let config = service
        .catalog_native0_creation(&captured)
        .unwrap()
        .unwrap();
    assert_eq!(config.configuration(), configuration);
    assert_eq!(config.limits(), limits);
    assert!(service
        .catalog_native0_creation(&CapturedCatalogOrigin { profile: None })
        .unwrap()
        .is_none());
    service
        .install_observer_factory_policy(NativeFactoryPolicy::ExistingManagerStandardCatalog)
        .unwrap();
    assert!(service
        .catalog_native0_creation(&captured)
        .unwrap()
        .is_none());
}
#[cfg(feature = "experimental-shellfox-native")]
#[test]
fn invalid_native0_policy_keeps_old_source_authority_disabled() {
    use shellfox_ghostty_vt::extension::{RestorationConfig, SnapshotLimits};
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    service
        .install_observer_factory_policy(NativeFactoryPolicy::ExistingManagerStandardCatalog)
        .unwrap();
    let limits = SnapshotLimits {
        max_encoded_bytes: 4 * 1024 * 1024 + 1,
        ..SnapshotLimits::default()
    };
    assert!(service
        .install_observer_factory_policy(NativeFactoryPolicy::ExistingManagerNative0Catalog {
            configuration: RestorationConfig {
                runtime_image_storage_limit: 10_000_000,
                runtime_resize_pull_scrollback: false
            },
            limits,
        })
        .is_err());
    assert_eq!(service.factory_policy.policy, NativeFactoryPolicy::Disabled);
}
