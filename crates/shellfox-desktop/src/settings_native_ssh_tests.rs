use super::{
    api::{self, SettingsServices},
    native_gateway::{Gateway, Reply},
    native_services::NativeServices,
};
use shellfox_service::{
    native::{NativeAction, NativeResponse},
    ssh,
};
use std::sync::{Arc, Mutex};
enum Outcome {
    Profiles,
    Selected,
    Opened,
    Unavailable,
    Wrong,
}
struct Fixture {
    requests: Mutex<Vec<NativeAction>>,
    outcome: Outcome,
}
impl Gateway for Fixture {
    fn execute(&self, request: NativeAction) -> Reply<'_> {
        self.requests.lock().unwrap().push(request);
        Box::pin(async {
            Ok(match self.outcome {
                Outcome::Profiles => NativeResponse::Ssh(ssh::Response::Profiles(vec![])),
                Outcome::Selected => {
                    NativeResponse::Ssh(ssh::Response::SelectedKey(ssh::SelectedKeyInfo {
                        path: std::env::current_dir()
                            .unwrap()
                            .join("tmp/owned-selected-key")
                            .to_str()
                            .unwrap()
                            .into(),
                        metadata: ssh::KeyMetadata {
                            format: ssh::KeyFormat::OpenSsh,
                            encrypted: true,
                            algorithm: None,
                            limitation: None,
                        },
                        cryptographically_validated: false,
                    }))
                }
                Outcome::Opened => NativeResponse::SshOpened(opened()),
                Outcome::Unavailable => {
                    return Err(anyhow::anyhow!(
                        "SSH services have not been installed by the owning bootstrap."
                    ));
                }
                Outcome::Wrong => NativeResponse::Applied,
            })
        })
    }
}
fn fixture(outcome: Outcome) -> (NativeServices, Arc<Fixture>) {
    let gateway = Arc::new(Fixture {
        requests: Mutex::new(vec![]),
        outcome,
    });
    (NativeServices::injected(gateway.clone()), gateway)
}
fn run(service: &NativeServices, request: api::Request) -> anyhow::Result<api::Response> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(service.request(request))
}
fn input(password: api::Password) -> api::SshInput {
    api::SshInput {
        id: "00000000-0000-4000-8000-000000000004".into(),
        name: "Owned fixture".into(),
        host: "example.invalid".into(),
        port: 2222,
        user: "fixture".into(),
        password,
        key_file: None,
        remote_cwd: Some("/work".into()),
    }
}
fn opened() -> ssh::OpenedProfile {
    ssh::OpenedProfile {
        snapshot: shellfox_core::Snapshot {
            revision: 9,
            sessions: vec![],
            preferences: Default::default(),
            selection: Default::default(),
        },
        connection: ssh::ConnectionStatus {
            runtime: None,
            connection: "00000000-0000-4000-8000-000000000005".parse().unwrap(),
            profile: "00000000-0000-4000-8000-000000000004".parse().unwrap(),
            tab: "00000000-0000-4000-8000-000000000006".parse().unwrap(),
            state: ssh::State::Preparing,
        },
    }
}
#[test]
fn list_and_delete_are_actual_typed_owner_requests() {
    let (service, gateway) = fixture(Outcome::Profiles);
    assert!(matches!(
        run(&service, api::Request::SshList).unwrap(),
        api::Response::SshProfiles(_)
    ));
    run(
        &service,
        api::Request::SshDelete(input(api::Password::Preserve).id),
    )
    .unwrap();
    let requests = gateway.requests.lock().unwrap();
    assert!(matches!(
        &requests[0],
        NativeAction::Ssh(ssh::Request::List)
    ));
    assert!(
        matches!(&requests[1],NativeAction::Ssh(ssh::Request::Delete{id})if id.to_string()=="00000000-0000-4000-8000-000000000004")
    );
}
#[test]
fn save_preserve_clear_and_secret_are_distinct_and_redacted() {
    let (service, gateway) = fixture(Outcome::Profiles);
    run(
        &service,
        api::Request::SshSave(input(api::Password::Preserve)),
    )
    .unwrap();
    run(&service, api::Request::SshSave(input(api::Password::Clear))).unwrap();
    let secret = "owned-synthetic-secret";
    run(
        &service,
        api::Request::SshSave(input(api::Password::Replace(secret.into()))),
    )
    .unwrap();
    let requests = gateway.requests.lock().unwrap();
    assert!(
        matches!(&requests[0],NativeAction::Ssh(ssh::Request::Save(input))if matches!(input.password,ssh::Password::Keep))
    );
    assert!(
        matches!(&requests[1],NativeAction::Ssh(ssh::Request::Save(input))if matches!(input.password,ssh::Password::Clear))
    );
    assert!(
        matches!(&requests[2],NativeAction::Ssh(ssh::Request::Save(input))if matches!(input.password,ssh::Password::Set(_)))
    );
    assert!(!format!("{requests:?}").contains(secret));
}
#[test]
fn selected_key_is_validated_without_chooser_rpc_or_crypto_success_claim() {
    let (service, gateway) = fixture(Outcome::Selected);
    let path = std::env::current_dir()
        .unwrap()
        .join("tmp/owned-key-not-read-by-test");
    let selected = ssh::SelectedKey::new(path).unwrap();
    let result = run(&service, api::Request::ValidateSshKey(selected)).unwrap();
    assert!(
        matches!(result,api::Response::KeyFile(Some(path))if path.ends_with("owned-selected-key"))
    );
    assert!(matches!(
        &gateway.requests.lock().unwrap()[0],
        NativeAction::Ssh(ssh::Request::ValidateKey { .. })
    ));
    assert_eq!(gateway.requests.lock().unwrap().len(), 1);
}
#[test]
fn source_open_keeps_original_intent_and_does_not_global_select_or_claim_ready() {
    let (_, gateway) = fixture(Outcome::Opened);
    let request_id = shellfox_core::SessionId::new();
    let request = ssh::OpenProfile {
        request_id,
        profile: input(api::Password::Preserve).id.parse().unwrap(),
        cols: 80,
        rows: 24,
    };
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(super::ssh_open::execute_open(gateway.as_ref(), request))
        .unwrap();
    assert!(matches!(result.connection.state, ssh::State::Preparing));
    assert!(result.snapshot.selection.session_id.is_none());
    assert!(
        matches!(&gateway.requests.lock().unwrap()[0],NativeAction::SshOpen(input)if input.request_id==request_id&&input.cols==80&&input.rows==24)
    );
}
#[test]
fn missing_native_installation_and_wrong_response_remain_errors() {
    for outcome in [Outcome::Unavailable, Outcome::Wrong] {
        let (service, _) = fixture(outcome);
        assert!(run(&service, api::Request::SshList).is_err());
    }
}
#[test]
fn invalid_profile_identifier_never_reaches_owner() {
    let (service, gateway) = fixture(Outcome::Profiles);
    assert!(run(&service, api::Request::SshDelete("not-an-id".into())).is_err());
    assert!(gateway.requests.lock().unwrap().is_empty());
}
