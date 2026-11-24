use super::super::OwnedSystemFixture as Fixture;
use super::super::*;
use crate::{
    native::{NativeAction, NativeResponse},
    owner::ServiceOwner,
};
use shellfox_protocol::CancellationToken;

#[tokio::test(flavor = "current_thread")]
async fn actual_owner_client_routes_system_settings_with_real_provider_and_no_boot_writes() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let data = fixture.root.join("owner-data");
    std::fs::create_dir(&data).unwrap();
    let mut owner = ServiceOwner::start(
        data,
        fixture.root.join("package/Shellfox.exe"),
        CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    assert!(client
        .native(NativeAction::System(SystemNativeRequest::Status {
            scope: SystemScope::User
        }))
        .await
        .is_err());
    client
        .install_system_native(fixture.backend(
            Vec::new(),
            SystemInstallPolicy {
                allow_user_install: true,
                allow_open: false,
            },
        ))
        .await
        .unwrap();
    assert!(!fixture.plan.directory.exists());
    let NativeResponse::System(SystemNativeResponse::Status(status)) = client
        .native(NativeAction::System(SystemNativeRequest::Status {
            scope: SystemScope::User,
        }))
        .await
        .unwrap()
    else {
        panic!("owner system status")
    };
    assert!(!status.facts.cli.as_ref().unwrap().installed);
    assert_eq!(status.capabilities.cli.install, SystemCapability::Available);
    let NativeResponse::System(SystemNativeResponse::Cli(state)) = client
        .native(NativeAction::System(SystemNativeRequest::SetCli {
            scope: SystemScope::User,
            installed: true,
        }))
        .await
        .unwrap()
    else {
        panic!("owner CLI result")
    };
    assert!(state.installed);
    client.native(NativeAction::DetachAll).await.unwrap();
    assert!(fixture.opener.0.lock().unwrap().is_empty());
    owner.shutdown().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn lost_accepted_reply_does_not_retry_an_external_installation_change() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let data = fixture.root.join("owner-data");
    std::fs::create_dir(&data).unwrap();
    let mut owner = ServiceOwner::start(
        data,
        fixture.root.join("package/Shellfox.exe"),
        CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    client
        .install_system_native(fixture.backend(
            Vec::new(),
            SystemInstallPolicy {
                allow_user_install: true,
                allow_open: false,
            },
        ))
        .await
        .unwrap();
    let accepted = client
        .try_native(NativeAction::System(SystemNativeRequest::SetCli {
            scope: SystemScope::User,
            installed: true,
        }))
        .unwrap();
    drop(accepted);
    let NativeResponse::System(SystemNativeResponse::Status(status)) = client
        .native(NativeAction::System(SystemNativeRequest::Status {
            scope: SystemScope::User,
        }))
        .await
        .unwrap()
    else {
        panic!("owner system status")
    };
    assert!(status.facts.cli.as_ref().unwrap().installed);
    owner.shutdown().await.unwrap();
}
#[test]
fn no_system_settings_method_is_deserializable_from_private_or_remote_wire() {
    for method in [
        "system",
        "getSystemStatus",
        "setCliIntegration",
        "setSystemExplorer",
    ] {
        let value =
            serde_json::json!({"method":method,"payload":{"scope":"user","installed":true}});
        let mut envelope = value.clone();
        envelope["clientId"] = serde_json::json!("12345678-1234-4234-8234-123456789abc");
        assert!(
            crate::local::PrivateRequest::decode(&serde_json::to_vec(&envelope).unwrap()).is_err()
        );
        assert!(serde_json::from_value::<shellfox_protocol::RemoteCommand>(value).is_err());
    }
}
