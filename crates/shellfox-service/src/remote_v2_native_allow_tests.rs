#![cfg(windows)]
use super::*;
use shellfox_core::{CreateSession, ProfileId, SessionId, TerminalIdentity};
#[test]
#[ignore = "requires explicit separately built owned supervisor; uses only its original private PTY, never an alternate token"]
fn effective_delivery_uses_actual_native_token_and_never_overrides_admin_inheritance() {
    let data = crate::tests::scratch();
    let supervisor = std::path::PathBuf::from(
        std::env::var_os("SHELLFOX_NATIVE_TEST_SUPERVISOR").expect("explicit owned supervisor"),
    );
    let mut service = Service::open_owned(data.path(), &supervisor).unwrap();
    let request = SessionId::new();
    let snapshot = service
        .create_application_session(CreateSession {
            request_id: request,
            title: Some("owned V2 share fixture".into()),
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } =
        service.host().creation_request_receipt(request).unwrap()
    else {
        panic!("receipt")
    };
    let tab = snapshot
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap()
        .tabs[0]
        .id;
    service.start_terminal(tab).unwrap();
    let identity = service.terminals[&tab].runtime.identity();
    let actual = service.terminals[&tab].runtime.launch_privilege_facts();
    assert!(
        runtime::effective_policy(&service, identity).is_none(),
        "actual Normal alone is never a source grant"
    );
    service
        .install_observer_factory_policy(
            crate::native_sharing::NativeFactoryPolicy::ExistingManagerStandardCatalog,
        )
        .unwrap();
    assert!(
        service.observer_delivery_policy(identity).is_some(),
        "original trusted catalog selection was captured at birth"
    );
    match actual.privilege {
        shellfox_runtime::LaunchPrivilege::Normal => {
            assert!(runtime::effective_policy(&service, identity).is_some())
        }
        shellfox_runtime::LaunchPrivilege::Elevated
        | shellfox_runtime::LaunchPrivilege::Unknown => {
            assert!(runtime::effective_policy(&service, identity).is_none())
        }
    }
    let state = RemoteState::new(Default::default(), None, None);
    if actual.privilege == shellfox_runtime::LaunchPrivilege::Elevated {
        let snapshot = catalog::snapshot(&state, &service).unwrap();
        let v2::CatalogEntry::Session { value } = &snapshot.entries[0] else { panic!("saved session metadata") };
        assert!(value.tabs.is_empty(), "actual elevated TAB identities stay local-only while saved session metadata remains authorized");
    }
    let mut stale = identity;
    stale.generation = shellfox_core::Generation::new();
    assert!(runtime::effective_policy(&service, stale).is_none());
    service.disable_observer_factory_policy().unwrap();
    assert!(runtime::effective_policy(&service, identity).is_none());
    service
        .stop_terminal(TerminalIdentity {
            tab_id: tab,
            generation: identity.generation,
        })
        .unwrap();
}
