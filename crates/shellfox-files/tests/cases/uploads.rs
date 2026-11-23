use crate::support::Fixture;
use shellfox_files::*;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use uuid::Uuid;

fn actor() -> UploadActor {
    UploadActor::new(
        super::streams::actor(),
        TerminalUploadIdentity {
            tab_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
            control_epoch: 1,
        },
        UploadEnvironment::Local,
    )
    .unwrap()
}
fn data_entries(path: &Path) -> usize {
    fs::read_dir(path)
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().file_name() != ".shellfox-transfer-meta")
        .count()
}
fn allow(_: &UploadActor) -> Result<()> {
    Ok(())
}

#[test]
fn private_uploads_stream_eight_mib_and_preserve_unverified_prior_session_artifacts() {
    let f = Fixture::new();
    let root = f.root.join("inbox");
    let actor = actor();
    let mut inbox = UploadInbox::open(&root, UploadLimits::default()).unwrap();
    let data = vec![b'z'; TRANSFER_TEXT_BYTES as usize];
    let started = inbox
        .begin(
            &actor,
            &BeginUpload {
                file_name: "../../CON.txt. ".into(),
                size: data.len() as u64,
            },
            &allow,
        )
        .unwrap();
    for (index, bytes) in data.chunks(CHUNK_BYTES).enumerate() {
        inbox
            .write_chunk(
                &actor,
                &UploadChunk {
                    upload_id: started.upload_id,
                    offset: (index * CHUNK_BYTES) as u64,
                    data: bytes.to_vec(),
                },
                &allow,
            )
            .unwrap();
    }
    let id = UploadIdentity {
        upload_id: started.upload_id,
    };
    let finished = inbox.finish(&actor, &id, &allow, &LocalUploadPath).unwrap();
    assert_eq!(finished.file_name, "file_CON.txt");
    assert_eq!(fs::read(&finished.path).unwrap(), data);
    assert_eq!(
        inbox
            .finish(&actor, &id, &allow, &LocalUploadPath)
            .unwrap()
            .revision,
        finished.revision
    );
    assert_eq!(inbox.usage().saved, 1);
    assert_eq!(inbox.usage().active, 0);
    drop(inbox);
    let orphan = root.join(Uuid::new_v4().to_string());
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join(".partial"), "abandoned").unwrap();
    let restarted = UploadInbox::open(&root, UploadLimits::default()).unwrap();
    assert_eq!(restarted.usage().saved, 0);
    assert_eq!(fs::read(orphan.join(".partial")).unwrap(), b"abandoned");
    assert_eq!(fs::read(finished.path).unwrap(), data);
}

#[test]
fn uploads_require_same_generation_and_control_and_cancel_after_revocation() {
    let f = Fixture::new();
    let mut inbox = UploadInbox::open(f.root.join("inbox"), UploadLimits::default()).unwrap();
    let owner = actor();
    let approved = AtomicBool::new(true);
    let auth = |_: &UploadActor| {
        if approved.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(FileError {
                code: ErrorCode::AccessDenied,
                message: "revoked or elevated".into(),
            })
        }
    };
    let started = inbox
        .begin(
            &owner,
            &BeginUpload {
                file_name: "x".into(),
                size: 1,
            },
            &auth,
        )
        .unwrap();
    let mut stale = owner.clone();
    stale.terminal.control_epoch += 1;
    let chunk = UploadChunk {
        upload_id: started.upload_id,
        offset: 0,
        data: vec![0],
    };
    assert_eq!(
        inbox.write_chunk(&stale, &chunk, &auth).unwrap_err().code,
        ErrorCode::NotFound
    );
    inbox.write_chunk(&owner, &chunk, &auth).unwrap();
    approved.store(false, Ordering::Release);
    assert_eq!(
        inbox
            .finish(
                &owner,
                &UploadIdentity {
                    upload_id: started.upload_id
                },
                &auth,
                &LocalUploadPath
            )
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert_eq!(inbox.usage().active, 0);
    assert_eq!(data_entries(inbox.root_path()), 0);
    approved.store(true, Ordering::Release);
    let started = inbox
        .begin(
            &owner,
            &BeginUpload {
                file_name: "cancel".into(),
                size: 1,
            },
            &auth,
        )
        .unwrap();
    approved.store(false, Ordering::Release);
    // An owner may explicitly discard its partial after losing control or approval.
    inbox
        .cancel(
            &owner,
            &UploadIdentity {
                upload_id: started.upload_id,
            },
        )
        .unwrap();
    assert_eq!(data_entries(inbox.root_path()), 0);
}

struct RevokeOnTranslation<'a>(&'a AtomicBool);
impl UploadPathResolver for RevokeOnTranslation<'_> {
    fn resolve(&self, _: &Path, _: &UploadActor) -> Result<String> {
        self.0.store(false, Ordering::Release);
        Ok("/mnt/owned/file".into())
    }
}
#[test]
fn path_translation_revalidates_approval_before_publication_and_never_falls_back_from_wsl() {
    let f = Fixture::new();
    let mut inbox = UploadInbox::open(f.root.join("inbox"), UploadLimits::default()).unwrap();
    let mut owner = actor();
    owner.environment = UploadEnvironment::Wsl {
        distro: "SelectedTestDistro".into(),
    };
    let approved = AtomicBool::new(true);
    let auth = |_: &UploadActor| {
        if approved.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(FileError {
                code: ErrorCode::AccessDenied,
                message: "revoked".into(),
            })
        }
    };
    let start = inbox
        .begin(
            &owner,
            &BeginUpload {
                file_name: "empty".into(),
                size: 0,
            },
            &auth,
        )
        .unwrap();
    assert_eq!(
        inbox
            .finish(
                &owner,
                &UploadIdentity {
                    upload_id: start.upload_id
                },
                &auth,
                &RevokeOnTranslation(&approved)
            )
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert_eq!(data_entries(inbox.root_path()), 0);
    approved.store(true, Ordering::Release);
    let start = inbox
        .begin(
            &owner,
            &BeginUpload {
                file_name: "empty".into(),
                size: 0,
            },
            &auth,
        )
        .unwrap();
    assert_eq!(
        inbox
            .finish(
                &owner,
                &UploadIdentity {
                    upload_id: start.upload_id
                },
                &auth,
                &LocalUploadPath
            )
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(data_entries(inbox.root_path()), 0);
}

#[test]
fn upload_limit_expiry_and_strict_restart_do_not_delete_unexpected_files() {
    let f = Fixture::new();
    let root = f.root.join("inbox");
    let mut inbox = UploadInbox::open(
        &root,
        UploadLimits {
            max_file_bytes: 1,
            active_ttl: Duration::from_millis(2),
            saved_ttl: Duration::from_millis(2),
        },
    )
    .unwrap();
    let owner = actor();
    assert_eq!(
        inbox
            .begin(
                &owner,
                &BeginUpload {
                    file_name: "large".into(),
                    size: 2
                },
                &allow
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    inbox
        .begin(
            &owner,
            &BeginUpload {
                file_name: "idle".into(),
                size: 1,
            },
            &allow,
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(10));
    inbox.sweep(&allow).unwrap();
    assert_eq!(inbox.usage().active, 0);
    drop(inbox);
    let mut inbox = UploadInbox::open(
        &root,
        UploadLimits {
            max_file_bytes: 1,
            active_ttl: Duration::from_secs(300),
            saved_ttl: Duration::from_millis(2),
        },
    )
    .unwrap();
    let start = inbox
        .begin(
            &owner,
            &BeginUpload {
                file_name: "saved".into(),
                size: 0,
            },
            &allow,
        )
        .unwrap();
    inbox
        .finish(
            &owner,
            &UploadIdentity {
                upload_id: start.upload_id,
            },
            &allow,
            &LocalUploadPath,
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(10));
    inbox.sweep(&allow).unwrap();
    assert_eq!(data_entries(&root), 0);
    drop(inbox);
    fs::write(root.join("unexpected"), "keep").unwrap();
    assert!(UploadInbox::open(&root, UploadLimits::default()).is_err());
    assert_eq!(fs::read(root.join("unexpected")).unwrap(), b"keep");
}

#[test]
fn upload_sanitization_matches_existing_path_and_device_fixtures() {
    for (input, expected) in [
        ("../../foo.txt", "foo.txt"),
        ("C:\\evil\\NUL.jpg", "file_NUL.jpg"),
        (". .", "file"),
        ("a.txt:evil", "a.txt_evil"),
        ("foo.  ", "foo"),
        ("\u{202e}evil\0.png", "_evil_.png"),
        ("$(touch x);.sh", "__touch x__.sh"),
    ] {
        assert_eq!(sanitize_upload_name(input), expected);
    }
}
