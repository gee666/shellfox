//! Real Go-owned Host actor/SQLite plus the existing owned legacy-inspection fixture.
use shellfox_test_support::native_metadata as metadata;
#[path = "support/native_bootstrap_owned.rs"]
mod support;
use anyhow::Result;
use shellfox_protocol::CancellationToken;
use shellfox_remote::{Approval, ApprovalControl, PrivateApprovalStore, ViewPermission};
use shellfox_transport::incoming::PeerIdentity;
use std::sync::Arc;
use support::*;
fn app<T>(value: std::result::Result<T, shellfox_protocol::AppError>) -> Result<T> {
    value.map_err(|e| anyhow::anyhow!(e.message))
}
#[test]
fn real_go_owned_actor_approval_commit_revoke_and_restart() -> Result<()> {
    let fixture = Fixture::new()?;
    let data = fixture.data("approval-host");
    let saved = cycle(&fixture, &data, |client, _| async move {
        // This fixture only inspects legacy paths. SQL authority uses the actual
        // ready Go-owned actor, whose handler rechecks its original lease.
        let metadata = metadata::Fixture::new();
        let deny = metadata.lost.clone();
        let repository = client.approval_repository(deny.clone());
        let store = Arc::new(PrivateApprovalStore::retained_native(
            metadata.files.clone(),
            deny,
        ));
        app(store.bind_repository(repository.clone()))?;
        app(store.initialize_read_only())?;
        let control = app(ApprovalControl::load(&store).await)?;
        let peer = PeerIdentity {
            id: "n-owned-offline".into(),
            hostname: "owned-offline".into(),
        };
        let approval = Approval {
            peer_id: peer.id.clone(),
            hostname: peer.hostname.clone(),
            approved_at_millis: 1,
            permission: ViewPermission::Write,
            file_roots: vec![],
        };
        app(control.approve(&store, approval.clone()).await)?;
        let actor = app(control.authorize(&peer, uuid::Uuid::new_v4(), 1))?;
        app(control.revoke(&store, &peer.id).await)?;
        anyhow::ensure!(
            actor.with_authorized(None, || Ok(())).is_err(),
            "revoked epoch regained authority"
        );
        anyhow::ensure!(
            app(repository.load().await)?.approvals.is_empty(),
            "revocation did not commit"
        );
        app(control.approve(&store, approval).await)?;
        let snapshot = app(repository.load().await)?;
        // Mirrors the existing production RemoteRuntime closing gate, no fake exit proof.
        app(control.set_enabled(false))?;
        let legacy = metadata
            .files
            .private_child(std::ffi::OsStr::new("remote-v1"))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        metadata::Fixture::seed(&legacy, "write-pending", b"owned legacy barrier, preserve");
        anyhow::ensure!(
            ApprovalControl::load(&store).await.is_err(),
            "legacy marker gained SQL authority"
        );
        anyhow::ensure!(
            legacy
                .read_private(std::ffi::OsStr::new("write-pending"), 1024)?
                .bytes
                == b"owned legacy barrier, preserve",
            "legacy barrier was changed"
        );
        anyhow::ensure!(
            !metadata
                .control_path()
                .join("remote-v1/approvals.json")
                .exists(),
            "unexpected JSON authority writer"
        );
        drop(legacy);
        drop(store);
        drop(metadata);
        Ok(snapshot)
    })?;
    cycle(&fixture, &data, move |client, _| async move {
        let repository = client.approval_repository(CancellationToken::new());
        let loaded = app(repository.load().await)?;
        anyhow::ensure!(loaded == saved, "restart lost committed approvals");
        let revoked = app(repository.commit(loaded.revision, vec![]).await)?;
        anyhow::ensure!(
            revoked.approvals.is_empty(),
            "restart revoke did not commit"
        );
        Ok(())
    })?;
    Ok(())
}
