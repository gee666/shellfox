use super::*;
#[test]
fn approved_legacy_record_mints_explicit_account_control_without_opening_or_enumerating_old_roots()
{
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    #[cfg(windows)]
    let root = std::path::PathBuf::from(
        root.to_str()
            .unwrap()
            .strip_prefix(r"\\?\")
            .unwrap_or(root.to_str().unwrap()),
    );
    let data = tempfile::Builder::new()
        .prefix("account-policy-")
        .tempdir_in(root)
        .unwrap();
    let control = ApprovalControl::default();
    let peer = PeerIdentity {
        id: "n-account".into(),
        hostname: "owned".into(),
    };
    let pending = ClientId::new_v4();
    assert_eq!(
        control.authorize(&peer, pending, 1).err().unwrap().code,
        ErrorCode::ApprovalRequired
    );
    assert!(control.state().unwrap().approvals.is_empty());
    let deleted_legacy_root = data.path().join("no-longer-exists");
    assert!(!deleted_legacy_root.exists());
    control
        .install(Approval {
            peer_id: peer.id.clone(),
            hostname: peer.hostname.clone(),
            approved_at_millis: 1,
            permission: ViewPermission::ReadOnly,
            file_roots: vec![deleted_legacy_root],
        })
        .unwrap();
    let actor = control.authorize(&peer, pending, 2).unwrap();
    assert_eq!(actor.permission, ViewPermission::Write);
    assert_eq!(actor.file_capabilities().len(), 1);
    let cap = &actor.file_capabilities()[0];
    assert_eq!(
        cap.capability_kind(),
        shellfox_files::CapabilityKind::ApprovedFullAccount
    );
    assert!(cap.root_path().is_none());
    let outside = data.path().join("outside-old-root.txt");
    std::fs::write(&outside, b"actual account fixture").unwrap();
    let read = actor
        .with_authorized(None, || {
            crate::file_ops::read(&actor, outside.to_str().unwrap())
        })
        .unwrap();
    assert_eq!(read["text"], "actual account fixture");
    let request = shellfox_protocol::files::WriteFile {
        session_id: None,
        tab_id: None,
        path: outside.to_str().unwrap().into(),
        text: "updated".into(),
        revision: read["revision"].as_str().unwrap().into(),
    };
    actor
        .with_authorized(Some(&RemoteCommand::WriteFile(request.clone())), || {
            crate::file_ops::write(&actor, &request)
        })
        .unwrap();
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "updated");
    assert_eq!(
        actor
            .with_authorized(None, || crate::file_ops::write(&actor, &request))
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    control.reject_inner(&peer.id, true).unwrap();
    assert_eq!(
        actor
            .with_authorized(None, || crate::file_ops::read(
                &actor,
                outside.to_str().unwrap()
            ))
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert!(
        cap.read_file(&shellfox_files::Location {
            path: outside.to_str().unwrap().into()
        })
        .is_err(),
        "revocation also fences the retained explicit account service"
    );
    assert!(control.authorize(&peer, pending, 3).is_err());
}

#[tokio::test]
async fn failed_durable_revocation_permanently_denies_existing_authority() {
    let control = ApprovalControl::default();
    let peer = PeerIdentity {
        id: "n-memory".into(),
        hostname: "fixture".into(),
    };
    let approval = Approval {
        peer_id: peer.id.clone(),
        hostname: peer.hostname.clone(),
        approved_at_millis: 1,
        permission: ViewPermission::Write,
        file_roots: Vec::new(),
    };
    control.install(approval.clone()).unwrap();
    let actor = control.authorize(&peer, ClientId::new_v4(), 1).unwrap();
    // This legacy store cannot commit a revocation to Host SQLite.
    let store = PrivateApprovalStore::retained_native(
        Arc::new(FileService::approved_full_account()),
        shellfox_protocol::CancellationToken::new(),
    );
    assert_eq!(
        control.revoke(&store, &peer.id).await.unwrap_err().code,
        ErrorCode::StorageFailed
    );
    assert!(control.state().unwrap().approvals.is_empty());
    assert!(actor
        .with_authorized::<()>(None, || panic!(
            "gated authority must not reach input/file/metadata effects"
        ))
        .is_err());
    assert_eq!(
        control.set_enabled(true).unwrap_err().code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        control.install(approval).unwrap_err().code,
        ErrorCode::Unsupported
    );
    assert!(control.authorize(&peer, ClientId::new_v4(), 2).is_err());
}
