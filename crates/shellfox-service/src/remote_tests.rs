use super::*;
use crate::native_metadata_fixture as native_metadata;
use shellfox_remote::{
    adapter::{ObservationFacts, SessionFacts, TabFacts},
    owner_channel, ApprovalControl, PrivateApprovalStore, RemoteHost,
};
use shellfox_transport::{
    incoming::PeerIdentity, HostOwnership, PermissionVerifier, PrivatePathKind, VerifiedHelper,
};
use std::path::{Path, PathBuf};

async fn dispatch(
    service: &mut Service,
    state: &mut RemoteState,
    command: OwnerCommand,
) -> Result<serde_json::Value, AppError> {
    let (sender, mut receiver) = owner_channel(1);
    let (result, ()) = tokio::join!(sender.request(command), async {
        receiver
            .recv()
            .await
            .unwrap()
            .dispatch(&mut state.bind(service));
    });
    result
}
pub(super) fn helper() -> VerifiedHelper {
    let path = PathBuf::from(std::env::var_os("SHELLFOX_OFFLINE_HELPER").expect("explicit helper"));
    let digest = std::env::var("SHELLFOX_OFFLINE_HELPER_SHA256").expect("explicit digest");
    assert_eq!(digest.len(), 64);
    let mut bytes = [0; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&digest[i * 2..i * 2 + 2], 16).unwrap();
    }
    VerifiedHelper::new(path, bytes).unwrap()
}
#[tokio::test]
#[ignore = "explicit verified offline Go helper required; no tsnet or login"]
async fn same_service_owner_metadata_adapter_and_revocation_end_to_end() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&parent).unwrap();
    let parent = std::fs::canonicalize(parent).unwrap();
    #[cfg(windows)]
    let parent = PathBuf::from(
        parent
            .to_str()
            .unwrap()
            .strip_prefix(r"\\?\")
            .unwrap_or(parent.to_str().unwrap()),
    );
    let scratch = tempfile::Builder::new()
        .prefix("service-remote-")
        .tempdir_in(parent)
        .unwrap();
    let helper = helper();
    PermissionVerifier::new(helper.clone())
        .protect(scratch.path(), PrivatePathKind::Directory)
        .await
        .unwrap();
    let ownership = HostOwnership::acquire(&helper, scratch.path())
        .await
        .unwrap()
        .unwrap();
    // Known pre-existing V2 fixture, not application approval WRITE acceptance.
    let metadata = native_metadata::Fixture::new();
    let directory = metadata
        .files
        .open_or_create_private_child(std::ffi::OsStr::new("remote-v1"))
        .unwrap();
    native_metadata::Fixture::seed(&directory,"approvals.json",br#"{"version":2,"approvals":[{"peerId":"n-service","hostname":"offline","approvedAtMillis":1,"policy":"control-full-account-v1"}]}"#);
    let store =
        PrivateApprovalStore::retained_native(metadata.files.clone(), metadata.lost.clone());
    store.initialize_read_only().unwrap();
    // Positive Service/Go metadata acceptance is PENDING native approval authority.
    let approvals = match ApprovalControl::load(&store).await {
        Ok(control) => control,
        Err(error) => {
            assert_eq!(error.code, ErrorCode::Unsupported);
            ownership.shutdown().await.unwrap();
            return;
        }
    };
    let actor = approvals
        .authorize(
            &PeerIdentity {
                id: "n-service".into(),
                hostname: "offline".into(),
            },
            wire::ClientId::new_v4(),
            1,
        )
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let created = service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: Some("Before".into()),
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: Default::default(),
        })
        .unwrap();
    let session = &created.sessions[0];
    let session_id = session.id;
    let tab = &session.tabs[0];
    let mut state = RemoteState::new(Default::default(), None, None);
    dispatch(
        &mut service,
        &mut state,
        OwnerCommand::Pair {
            actor: actor.clone(),
        },
    )
    .await
    .unwrap();
    let rename = RemoteCommand::RenameSession(wire::RenameSession {
        session_id,
        title: "After".into(),
    });
    assert_eq!(
        dispatch(
            &mut service,
            &mut state,
            OwnerCommand::Execute {
                actor: actor.clone(),
                command: rename.clone()
            }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        service.host().snapshot().unwrap().sessions[0].title,
        "Before"
    );
    // Explicit fixture observation: no runtime has been launched. This test
    // supplies known metadata projection facts, never a fake running process.
    assert_eq!(service.active_terminals(), 0);
    state.profiles = Some(serde_json::from_value(serde_json::json!({
        "profiles":[{"id":"login-shell","label":"Offline fixture","environment":"local","executable":std::env::current_exe().unwrap(),"args":[],"distro":null,"available":false,"unavailableReason":"No shell launch in this fixture","canTerminateDescendants":false}],
        "defaultProfileId":null,"lifetime":"app-owned","shellSurvival":false
    })).unwrap());
    state.facts.sessions.insert(
        session_id,
        SessionFacts {
            adapter_id: wire::AdapterId::EmbeddedPty,
            shell_id: wire::ShellId::LoginShell,
            window: None,
            can_focus: false,
            can_add_tab: false,
            control_reason: Some("No runtime in this fixture".into()),
            error: None,
            terminal_lifetime: Some(wire::TerminalLifetime::AppOwned),
            shell_survival: Some(false),
            tabs: [(
                tab.id,
                TabFacts {
                    created_at: tab
                        .created_at
                        .clone()
                        .expect("new tab has persisted creation evidence"),
                    lifecycle: wire::TabLifecycle::Closed,
                    observation: ObservationFacts::Unavailable {
                        reason: "No runtime launched".into(),
                    },
                    error: None,
                    generation: None,
                    terminal_kind: Some(wire::TerminalKind::Embedded),
                    exit_code: Some(None),
                },
            )]
            .into(),
        },
    );
    let response = dispatch(
        &mut service,
        &mut state,
        OwnerCommand::Execute {
            actor: actor.clone(),
            command: rename,
        },
    )
    .await
    .unwrap();
    let dto: wire::SessionDto = serde_json::from_value(response).unwrap();
    dto.validate().unwrap();
    assert_eq!(dto.title, "After");
    assert_eq!(
        service.host().snapshot().unwrap().sessions[0].title,
        "After"
    );
    let delete = RemoteCommand::DeleteSession(wire::DeleteSession { session_id });
    state.profiles.as_mut().unwrap().profiles[0].elevation = Some(wire::Elevation::Administrator);
    assert_eq!(
        dispatch(
            &mut service,
            &mut state,
            OwnerCommand::Execute {
                actor: actor.clone(),
                command: delete.clone()
            }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::AccessDenied
    );
    let profiles = dispatch(
        &mut service,
        &mut state,
        OwnerCommand::Execute {
            actor: actor.clone(),
            command: RemoteCommand::GetTerminalProfiles(wire::GetTerminalProfiles {}),
        },
    )
    .await
    .unwrap();
    assert!(profiles["profiles"].as_array().unwrap().is_empty());
    assert_eq!(service.host().snapshot().unwrap().sessions.len(), 1);
    state.profiles.as_mut().unwrap().profiles[0].elevation = None;
    assert_eq!(
        approvals
            .revoke(&store, &actor.peer_id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unsupported,
        "disk write remains gated; in-memory old epoch is still revoked"
    );
    assert_eq!(
        dispatch(
            &mut service,
            &mut state,
            OwnerCommand::Execute {
                actor: actor.clone(),
                command: delete
            }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::AccessDenied
    );
    assert_eq!(service.host().snapshot().unwrap().sessions.len(), 1);
    dispatch(&mut service, &mut state, OwnerCommand::Disconnect { actor })
        .await
        .unwrap();
    assert!(state.views.is_empty());
    service.shutdown().unwrap();
    drop(service);
    ownership.shutdown().await.unwrap();
}
// PENDING: original factory/journal success coverage is NOT retained as a pass.
// Native journals/uploads are gated. Actual BootGuard setup belongs to its
// private bootstrap fixture; these are only real Files leaf preference tests.
#[tokio::test]
async fn native_preference_leaf_roundtrip_does_not_initialize_journal_or_uploads() {
    let metadata = native_metadata::Fixture::new();
    let store = super::preferences::store::NativePreferenceStore::retained(
        metadata.files.clone(),
        metadata.lost.clone(),
    );
    let value = super::preferences::RemotePreferences {
        version: 1,
        enabled: true,
        hostname: "saved-offline".into(),
        peers: Vec::new(),
    };
    let _actual_durability = store.write(value).unwrap();
    assert_eq!(store.read().unwrap().unwrap().hostname, "saved-offline");
    let saved =
        std::fs::read_to_string(metadata.control_path().join("remote-settings-v1.json")).unwrap();
    for field in ["authKey", "loginUrl", "backendSecret"] {
        assert!(!saved.contains(field));
    }
    assert!(!metadata
        .control_path()
        .join("file-transfer-journal")
        .exists());
    assert!(!metadata.control_path().join("terminal-uploads").exists());
    store.preserve_retained().unwrap();
}
#[tokio::test]
async fn preference_read_checks_current_ownership_not_only_native_mutation_fence() {
    let metadata = native_metadata::Fixture::new();
    let store = super::preferences::store::NativePreferenceStore::retained(
        metadata.files.clone(),
        metadata.lost.clone(),
    );
    let value = super::preferences::RemotePreferences {
        version: 1,
        enabled: false,
        hostname: "before-loss".into(),
        peers: Vec::new(),
    };
    store.write(value.clone()).unwrap();
    let before = std::fs::read(metadata.control_path().join("remote-settings-v1.json")).unwrap();
    metadata.lost.cancel();
    assert!(store.read().is_err());
    assert!(store.write(value).is_err());
    assert_eq!(
        std::fs::read(metadata.control_path().join("remote-settings-v1.json")).unwrap(),
        before
    );
    store.preserve_retained().unwrap();
}
