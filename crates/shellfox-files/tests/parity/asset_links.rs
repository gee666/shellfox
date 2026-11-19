use crate::support::Fixture;
use shellfox_files::*;
use std::{fs, path::Path};
#[cfg(windows)]
#[path = "../cases/windows_junction.rs"]
mod junction;
fn link(from: &Path, to: &Path) {
    #[cfg(windows)]
    junction::create(from, to);
    #[cfg(unix)]
    std::os::unix::fs::symlink(from, to).unwrap();
}
#[test]
fn asset_and_private_upload_roots_reject_junction_or_symlink_contexts() {
    let f = Fixture::new();
    let outside = Fixture::new();
    f.write("page.html", "preview");
    outside.write("private.css", "secret");
    link(&outside.root, &f.root.join("linked"));
    assert_eq!(
        f.service
            .read_asset(&AssetRequest {
                path: "page.html".into(),
                asset_path: "linked/private.css".into()
            })
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert!(UploadInbox::open(f.root.join("linked"), UploadLimits::default()).is_err());
    assert_eq!(outside.read("private.css"), b"secret");
    assert_eq!(fs::read_dir(&outside.root).unwrap().count(), 1);
}
#[test]
fn upload_revocation_immediately_after_publish_rolls_back_only_its_identified_file() {
    let f = Fixture::new();
    let mut inbox = UploadInbox::open(f.root.join("inbox"), UploadLimits::default()).unwrap();
    let owner = UploadActor::new(
        super::streams::actor(),
        TerminalUploadIdentity {
            tab_id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
            control_epoch: 1,
        },
        UploadEnvironment::Local,
    )
    .unwrap();
    let allow = |_: &UploadActor| Ok(());
    let start = inbox
        .begin(
            &owner,
            &BeginUpload {
                file_name: "new".into(),
                size: 0,
            },
            &allow,
        )
        .unwrap();
    let published = inbox
        .root_path()
        .join(start.upload_id.to_string())
        .join("new");
    let policy = |_: &UploadActor| {
        if published.exists() {
            Err(FileError {
                code: ErrorCode::AccessDenied,
                message: "revoked at publication".into(),
            })
        } else {
            Ok(())
        }
    };
    assert_eq!(
        inbox
            .finish(
                &owner,
                &UploadIdentity {
                    upload_id: start.upload_id
                },
                &policy,
                &LocalUploadPath
            )
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert_eq!(inbox.usage().active, 0);
    assert_eq!(inbox.usage().saved, 0);
    assert!(!published.parent().unwrap().exists());
    let remaining: Vec<_> = fs::read_dir(inbox.root_path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(remaining, [".shellfox-transfer-meta"]);
}
