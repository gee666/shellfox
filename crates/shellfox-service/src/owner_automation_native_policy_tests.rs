use super::*;
use crate::NativeFactoryPolicy;
use shellfox_core::{CreateSession, Generation, ProfileId, SessionId};
use shellfox_terminal::{PtyConfig, ShellProfileId};
fn create(service: &Service, cwd: &std::path::Path) -> TabId {
    let request_id = SessionId::new();
    let snapshot = service
        .create_application_session(CreateSession {
            request_id,
            title: Some("owned catalog policy fixture".into()),
            cwd: cwd.to_str().unwrap().into(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } =
        service.host.creation_request_receipt(request_id).unwrap()
    else {
        panic!("missing original creation receipt")
    };
    snapshot
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap()
        .tabs[0]
        .id
}
#[test]
#[ignore = "explicit owned Windows supervisor; no UAC/network or fake privilege"]
fn eligibility_uses_captured_catalog_and_started_not_phase_marker_or_normal_alone() {
    let (scratch, mut owned, helper) = service();
    let service = &mut owned.0;
    let tab = create(service, scratch.path());
    let launch = service.host.terminal_launch(tab).unwrap();
    let generation = Generation::new();
    let identity = Identity {
        session_id: launch.session_id,
        tab_id: tab,
        generation,
    };
    let mut config = PtyConfig::new(service.generation_home(tab, generation).unwrap());
    config.profile = Some(ShellProfileId::Cmd);
    config.supervisor = Some(helper.clone());
    config.cwd = scratch.path().into();
    let origin = crate::native_sharing::capture_catalog_origin(&config);
    service
        .host
        .register_terminal(TerminalIdentity {
            tab_id: tab,
            generation,
        })
        .unwrap();
    let hooks = service.register_activity(identity, &config).unwrap();
    let runtime = shellfox_runtime::Runtime::start_with_hooks(identity, config, hooks).unwrap();
    service
        .terminals
        .insert(tab, terminals::OwnedTerminal::from_catalog(runtime, origin));
    service
        .install_observer_factory_policy(NativeFactoryPolicy::ExistingManagerStandardCatalog)
        .unwrap();
    assert!(
        service.observer_delivery_policy(identity).is_none(),
        "READY must come from actual Started receipt, not phase"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while service.terminals[&tab].startup.is_none() {
        service.drain(tab).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(service.terminals[&tab].startup.as_ref().unwrap().is_ok());
    let first = service.observer_delivery_policy(identity).unwrap();
    assert!(service
        .observer_delivery_policy(Identity {
            generation: Generation::new(),
            ..identity
        })
        .is_none());
    assert!(service.observer_delivery_policy(Identity {session_id:SessionId::new(),..identity}).is_none());
    // This is an eligibility assertion only. A real elevated/unknown token still
    // vetoes delivery in Musa's backend; no normal/elevated result is fabricated.
    let facts = service.terminals[&tab].runtime.launch_privilege_facts();
    assert_eq!(
        facts.purpose,
        shellfox_runtime::LaunchFactoryPurpose::OwnedPtyProgram
    );
    service
        .install_observer_factory_policy(NativeFactoryPolicy::Disabled)
        .unwrap();
    assert!(service.observer_delivery_policy(identity).is_none());
    service
        .install_observer_factory_policy(NativeFactoryPolicy::ExistingManagerStandardCatalog)
        .unwrap();
    assert!(service.observer_delivery_policy(identity).unwrap() > first);
    let raw = service
        .start_automation(input(&helper, scratch.path(), vec!["idle".into()]), 9)
        .unwrap();
    let raw_identity = service.automation.relays[&raw.id].feed.identity();
    assert!(
        service.observer_delivery_policy(raw_identity).is_none(),
        "Raw READY/Normal/Show cannot imply factory eligibility"
    );
    service.close_tab(tab).unwrap();
    service.shutdown().unwrap();
}
#[test]
#[ignore = "explicit owned Windows supervisor; private fixture only"]
fn successful_catalog_source_without_optin_and_failed_start_never_get_eligibility() {
    let (scratch, mut owned, _) = service();
    let service = &mut owned.0;
    let tab = create(service, scratch.path());
    service.attach(tab, 71, false).unwrap();
    let identity = service.terminals[&tab].runtime.identity();
    assert!(service.observer_delivery_policy(identity).is_none());
    service
        .install_observer_factory_policy(NativeFactoryPolicy::ExistingManagerStandardCatalog)
        .unwrap();
    assert!(service.observer_delivery_policy(identity).is_some());
    let missing = scratch.path().join("missing cwd");
    let failed = create(service, &missing);
    assert!(service.attach(failed, 72, false).is_err());
    let failed_identity = service.terminals[&failed].runtime.identity();
    assert!(service.terminals[&failed]
        .startup
        .as_ref()
        .is_some_and(|result| result.is_err()));
    assert!(service.observer_delivery_policy(failed_identity).is_none());
    service.close_tab(failed).unwrap();
    service.close_tab(tab).unwrap();
    service.shutdown().unwrap();
}
