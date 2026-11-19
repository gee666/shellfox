use crate::support::Fixture;
use shellfox_files::*;
use uuid::Uuid;

#[test]
fn a_second_inbox_cannot_recover_live_partials_or_bypass_the_active_quota() {
    let f = Fixture::new();
    let root = f.root.join("inbox");
    let mut first = UploadInbox::open(&root, UploadLimits::default()).unwrap();
    let actor = UploadActor::new(
        super::streams::actor(),
        TerminalUploadIdentity {
            tab_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
            control_epoch: 1,
        },
        UploadEnvironment::Local,
    )
    .unwrap();
    let allow = |_: &UploadActor| Ok(());
    let start = first
        .begin(
            &actor,
            &BeginUpload {
                file_name: "kept".into(),
                size: 0,
            },
            &allow,
        )
        .unwrap();
    assert!(UploadInbox::open(&root, UploadLimits::default()).is_err());
    let completed = first
        .finish(
            &actor,
            &UploadIdentity {
                upload_id: start.upload_id,
            },
            &allow,
            &LocalUploadPath,
        )
        .unwrap();
    first.dispose().unwrap();
    assert_eq!(
        first.sweep(&allow).unwrap_err().code,
        ErrorCode::AccessDenied
    );
    for _ in 1..UPLOAD_ENTRIES {
        std::fs::create_dir(root.join(Uuid::new_v4().to_string())).unwrap();
    }
    let mut second = UploadInbox::open(&root, UploadLimits::default()).unwrap();
    // Restart charges preserved entries to the quota without adopting them as owned uploads.
    assert_eq!(second.usage().saved, 0);
    let error = second
        .begin(
            &actor,
            &BeginUpload {
                file_name: "over-quota".into(),
                size: 0,
            },
            &allow,
        )
        .unwrap_err();
    assert_eq!(error.message, "Host upload inbox quota reached.");
    assert!(std::path::Path::new(&completed.path).is_file());
}
