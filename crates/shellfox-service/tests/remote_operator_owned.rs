//! Local operator parity on the actual retained Host; no peer/auth/terminal claim.
pub use shellfox_service::app_composition as application;
#[path = "support/native_bootstrap_remote_owned.rs"]
pub mod support;
use anyhow::{ensure, Context, Result};
use shellfox_service::{
    native::{NativeAction, NativeResponse},
    remote::{AuthKey, RemoteBootResources, RemoteRequest},
};
use std::path::PathBuf;

#[test]
fn headless_operator_request_keeps_auth_key_constructor_and_debug_redaction() {
    let key = "tskey-auth-SYNTHETIC_OPERATOR_TEST";
    let request = RemoteRequest::Login {
        auth_key: Some(AuthKey::new(key.into()).expect("validated test key")),
    };
    assert!(!format!("{request:?}").contains(key));
    let encoded = serde_json::to_vec(&request).expect("private payload");
    let decoded: RemoteRequest =
        serde_json::from_slice(&encoded).expect("ctor validated roundtrip");
    assert!(!format!("{decoded:?}").contains(key));
    assert!(serde_json::from_str::<RemoteRequest>(
        r#"{"operation":"login","authKey":"invalid-secret"}"#
    )
    .is_err());
    assert!(serde_json::from_str::<RemoteRequest>(r#"{"operation":"configure","enabled":true,"hostname":"owned","controlUrl":"http://127.0.0.1:1"}"#).is_err());
    assert!(serde_json::from_str::<RemoteRequest>(r#"{"operation":"installProvider"}"#).is_err());
    assert!(matches!(
        serde_json::from_str::<RemoteRequest>(r#"{"operation":"state"}"#)
            .expect("empty closed State"),
        RemoteRequest::State
    ));
    assert!(serde_json::from_str::<RemoteRequest>(
        r#"{"operation":"state","extra":"must-reject"}"#
    )
    .is_err());
    assert!(serde_json::from_str::<RemoteRequest>(
        r#"{"operation":"configure","enabled":false,"hostname":"one","hostname":"two"}"#
    )
    .is_err());
}
#[test]
fn headless_operator_and_native_use_same_manager_and_original_lifetime() -> Result<()> {
    let fixture = support::Fixture::new()?;
    let data = fixture.data("local-operator");
    let local_data = data.clone();
    let helper = fixture.helper.clone();
    let digest = fixture.digest;
    let supervisor = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-test-supervisor"));
    let resources = RemoteBootResources::planned(
        data.join("rust-service-v1/go-network/node-v1"),
        "operator-owned".into(),
    )?;
    let options = shellfox_service::local::RemoteBootOptions::native(
        supervisor,
        application::system_services(false),
        shellfox_service::system::SystemScope::User,
        resources,
    )?;
    support::cycle_remote(&fixture, &data, options, |owner, _| async move {
        let repository = owner.approval_repository(shellfox_protocol::CancellationToken::new());
        let local =
            shellfox_service::local::LocalClient::connect(&local_data, &helper, digest).await?;
        let initial = local.remote_control(RemoteRequest::State).await?;
        ensure!(!initial.enabled && initial.pending.is_empty() && initial.approvals.is_empty());
        let changed = local
            .remote_control(RemoteRequest::Configure {
                enabled: false,
                hostname: "operator-from-cli".into(),
            })
            .await?;
        ensure!(!changed.enabled && changed.hostname == "operator-from-cli");
        let reply = owner
            .native(NativeAction::Remote(RemoteRequest::State))
            .await?;
        let NativeResponse::Remote(state) = reply else {
            anyhow::bail!("Native operator absent")
        };
        ensure!(
            state.hostname == changed.hostname,
            "Local route did not reach original Manager"
        );
        owner
            .native(NativeAction::Remote(RemoteRequest::Configure {
                enabled: false,
                hostname: "operator-from-native".into(),
            }))
            .await?;
        ensure!(
            local.remote_control(RemoteRequest::State).await?.hostname == "operator-from-native"
        );
        let before = repository
            .load()
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
        let rejected = local
            .remote_control(RemoteRequest::Approve {
                peer_id: "n-not-authenticated".into(),
            })
            .await;
        let failure = rejected.expect_err("no pending WhoIs identity must deny");
        let failure = failure
            .downcast_ref::<shellfox_service::local::LocalRequestFailure>()
            .context("validated operation failure was lost")?;
        ensure!(failure.error.code == shellfox_protocol::ErrorCode::AccessDenied);
        let after = repository
            .load()
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
        ensure!(after == before, "Denied operator invented SQL approval");
        local.disconnect().await?;
        ensure!(
            local.remote_control(RemoteRequest::State).await.is_err(),
            "Closed caller lifetime remained usable"
        );
        Ok(())
    })?;
    Ok(())
}
