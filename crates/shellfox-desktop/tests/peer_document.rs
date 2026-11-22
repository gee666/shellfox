//! Real generic Manager client/witness + production adapter contract tests. Not native Go authentication/GUI acceptance.
#![cfg(feature = "experimental-shellfox-native")]
pub use shellfox_service::peer::consumer as connection;
pub use connection::PeerConnection;
mod host_client {
    pub(crate) mod peer {
        pub(crate) use shellfox_service::peer::admission::SourceAdmission;
        pub(crate) use crate::connection::PeerConnection;
        pub(crate) type Transport = crate::support::Fake;
    }
}
#[path = "../src/peer_document_backend.rs"]
mod backend;
#[path = "peer_document/support.rs"]
mod support;
use shellfox_files::{EditorCancellation, WriteText};
use shellfox_protocol::{Validate, manager_v2 as m};
use std::sync::Arc;
use support::*;
#[test]
fn admitted_peer_roundtrip_uses_chunks_cas_and_original_binding_for_eight_mib() {
    run(async {
        let mut bytes = vec![b'x'; 8 * 1024 * 1024];
        bytes[..3].copy_from_slice(b"\xef\xbb\xbf");
        let f = Fixture::new(bytes.clone()).await;
        assert_eq!(f.backend.home().await.unwrap(), "/peer/project");
        assert_eq!(
            f.backend.list("/peer/project").await.unwrap().entries[0].path,
            PATH
        );
        let read = f
            .backend
            .read(PATH, &EditorCancellation::default())
            .await
            .unwrap();
        assert_eq!(read.text.as_bytes(), bytes);
        assert_eq!(read.revision, "a".repeat(64));
        let mut text = read.text;
        text.replace_range(3..4, "z");
        let request = WriteText {
            path: read.path,
            revision: read.revision,
            text: text.clone(),
        };
        let result = f
            .backend
            .write(&request, &EditorCancellation::default())
            .await
            .unwrap();
        assert_eq!(result.path, PATH);
        assert_eq!(result.size, 8 * 1024 * 1024);
        let canonical = serde_json::to_value(f.client.binding().unwrap()).unwrap();
        let state = f.fake.0.lock().unwrap();
        assert!(state.bindings.iter().all(|binding| binding == &canonical));
        assert_eq!(state.body, text.as_bytes());
        assert_eq!(state.expected_revision, Some("a".repeat(64)));
        assert!(state.bindings.iter().all(|b| b == &state.bindings[0]));
        drop(state);
        assert_eq!(f.count("PAIR"), 1);
        assert_eq!(f.count("finishFileWrite"), 1);
    })
}
#[test]
fn original_source_admission_rejects_retirement_without_pair_or_reacquisition() {
    run(async {
        let f = Fixture::new(b"old".to_vec()).await;
        let before = f.fake.0.lock().unwrap().calls.len();
        f.client.reconnect(m::Id::new());
        assert!(
            f.backend
                .read(PATH, &EditorCancellation::default())
                .await
                .is_err()
        );
        assert_eq!(f.fake.0.lock().unwrap().calls.len(), before);
        assert_eq!(f.count("PAIR"), 1);
    })
}
#[test]
fn same_peer_label_and_serialized_binding_cannot_substitute_another_actual_client() {
    run(async {
        let a = Fixture::new(b"one".to_vec()).await;
        let b = Fixture::new(b"two".to_vec()).await;
        let witness = a.client.binding_witness().unwrap();
        let original = a.connection.admit_source_with(&witness).unwrap();
        assert!(b.connection.admit_source_with(&witness).is_err());
        let backend = backend::PeerDocumentBackend::new(b.connection.clone(), original).unwrap();
        let count = b.fake.0.lock().unwrap().calls.len();
        assert!(
            backend
                .read(PATH, &EditorCancellation::default())
                .await
                .is_err()
        );
        assert_eq!(b.fake.0.lock().unwrap().calls.len(), count);
    })
}
#[test]
fn unconfirmed_finish_is_sent_once_and_never_cancelled_retried_or_repaired() {
    run(async {
        let f = Fixture::new(b"old".to_vec()).await;
        f.fake.0.lock().unwrap().lose_finish = true;
        let request = WriteText {
            path: PATH.into(),
            revision: "a".repeat(64),
            text: "new".into(),
        };
        assert!(
            f.backend
                .write(&request, &EditorCancellation::default())
                .await
                .is_err()
        );
        let count = f.fake.0.lock().unwrap().calls.len();
        assert!(
            f.backend
                .write(&request, &EditorCancellation::default())
                .await
                .is_err()
        );
        assert_eq!(f.fake.0.lock().unwrap().calls.len(), count);
        assert_eq!(f.count("finishFileWrite"), 1);
        assert_eq!(f.count("cancelFileTransfer"), 0);
        assert_eq!(f.count("PAIR"), 1);
    })
}
#[test]
fn response_to_another_target_is_rejected_and_only_actual_read_clears_unknown_save() {
    run(async {
        let f = Fixture::new(b"old".to_vec()).await;
        f.fake.0.lock().unwrap().read_other_target = true;
        assert!(
            f.backend
                .read(PATH, &EditorCancellation::default())
                .await
                .is_err()
        );
        assert_eq!(f.count("readFileChunk"), 0);
        f.fake.0.lock().unwrap().read_other_target = false;
        f.fake.0.lock().unwrap().finish_other_target = true;
        let request = WriteText {
            path: PATH.into(),
            revision: "a".repeat(64),
            text: "new".into(),
        };
        assert!(
            f.backend
                .write(&request, &EditorCancellation::default())
                .await
                .is_err()
        );
        let count = f.count("beginFileWrite");
        assert!(
            f.backend
                .write(&request, &EditorCancellation::default())
                .await
                .is_err()
        );
        assert_eq!(f.count("beginFileWrite"), count);
        let read = f
            .backend
            .read(PATH, &EditorCancellation::default())
            .await
            .unwrap();
        assert_eq!(read.text, "new");
        f.fake.0.lock().unwrap().finish_other_target = false;
        assert!(
            f.backend
                .write(
                    &WriteText {
                        path: read.path,
                        revision: read.revision,
                        text: "confirmed".into()
                    },
                    &EditorCancellation::default()
                )
                .await
                .is_ok()
        );
    })
}
#[test]
fn post_response_actual_witness_fence_refuses_a_save_retired_during_finish() {
    run(async {
        let f = Fixture::new(b"old".to_vec()).await;
        f.fake.0.lock().unwrap().retire_on_finish = Some(Arc::downgrade(&f.client));
        assert!(
            f.backend
                .write(
                    &WriteText {
                        path: PATH.into(),
                        revision: "a".repeat(64),
                        text: "new".into()
                    },
                    &EditorCancellation::default()
                )
                .await
                .is_err()
        );
        assert_eq!(f.count("finishFileWrite"), 1);
        assert_eq!(f.count("cancelFileTransfer"), 0);
        let mut applied = false;
        assert!(
            f.backend
                .commit_if_current(|| {
                    applied = true;
                })
                .is_err()
        );
        assert!(!applied);
    })
}
#[test]
fn cancellation_keeps_original_transfer_and_never_finishes_or_selects_local_io() {
    run(async {
        let f = Fixture::new(b"old".to_vec()).await;
        let cancellation = EditorCancellation::default();
        f.fake.0.lock().unwrap().cancel_on_write = Some(cancellation.clone());
        assert!(
            f.backend
                .write(
                    &WriteText {
                        path: PATH.into(),
                        revision: "a".repeat(64),
                        text: "pending".into()
                    },
                    &cancellation
                )
                .await
                .is_err()
        );
        assert_eq!(f.count("finishFileWrite"), 0);
        assert_eq!(f.count("cancelFileTransfer"), 1);
        assert_eq!(f.fake.0.lock().unwrap().body, b"old");
        assert_eq!(f.count("PAIR"), 1);
        let witness = f.client.binding_witness().unwrap();
        assert!(witness.binding().validate().is_ok());
    })
}
