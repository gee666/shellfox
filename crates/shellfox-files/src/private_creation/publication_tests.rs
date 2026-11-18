use super::*;
use crate::private_creation::tests::Fixture;
use std::{
    fs,
    io::Write,
    panic::{catch_unwind, AssertUnwindSafe},
};
#[cfg(windows)]
fn descriptor(file: &File) -> usize {
    use std::os::windows::io::AsRawHandle;
    file.as_raw_handle() as usize
}
#[cfg(unix)]
fn descriptor(file: &File) -> usize {
    use std::os::fd::AsRawFd;
    file.as_raw_fd() as usize
}
fn prepare(f: &Fixture) -> OwnedPrivatePublication {
    prepare_private_publication_at(
        f.parent(),
        OsStr::new("prefs"),
        OsStr::new(".pending"),
        None,
    )
    .unwrap()
}
#[test]
fn owns_before_write_and_success_keeps_an_opaque_published_receipt_without_replay() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    assert_eq!(value.phase(), PrivatePublicationPhase::Preparing);
    assert!(f.path.join(".pending").exists());
    value.write_all(b"private metadata").unwrap();
    let id = value.stage.as_ref().unwrap().identity().clone();
    assert!(matches!(
        value.publish(),
        PrivatePublishOutcome::Published(_)
    ));
    assert_eq!(value.published_receipt().unwrap().identity(), &id);
    assert_eq!(fs::read(f.path.join("prefs")).unwrap(), b"private metadata");
    assert!(matches!(value.publish(), PrivatePublishOutcome::Refused(_)));
    assert!(value.write_all(b"retry").is_err());
}
#[test]
fn known_publish_sync_failure_is_not_reported_as_no_write_or_lost_custody() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    value.write_all(b"visible").unwrap();
    let result = value.publish_with_sync(|_| {
        Err(FileError::new(
            ErrorCode::StorageFailed,
            "injected durability failure",
        ))
    });
    assert!(matches!(
        result,
        PrivatePublishOutcome::Published(PrivateDurability::Unconfirmed)
    ));
    assert_eq!(
        value.phase(),
        PrivatePublicationPhase::PublishedDurabilityUnconfirmed
    );
    assert!(value.durability_error().is_some());
    assert!(value.published_receipt().is_some());
    assert_eq!(fs::read(f.path.join("prefs")).unwrap(), b"visible");
}
#[test]
fn uncertain_rename_never_replays_or_manufactures_a_published_receipt_from_a_read() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    value.write_all(b"possibly visible").unwrap();
    let result = value.publish_with_ops(
        |parent, source, target, file, replace| {
            io::rename(parent, source, target, file, replace)?;
            Err(FileError::new(
                ErrorCode::StorageFailed,
                "injected lost rename result",
            ))
        },
        |_| Ok(()),
    );
    assert!(matches!(result, PrivatePublishOutcome::Uncertain(_)));
    assert_eq!(value.phase(), PrivatePublicationPhase::Uncertain);
    assert!(value.published_receipt().is_none());
    let read = crate::bounded_private_read_at(f.parent(), OsStr::new("prefs"), 100).unwrap();
    assert_eq!(read.bytes, b"possibly visible");
    assert!(value.published_receipt().is_none());
    assert!(matches!(value.publish(), PrivatePublishOutcome::Refused(_)));
    assert!(matches!(
        value.cleanup().unwrap(),
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: true
        }
    ));
    value.preserve_stale();
    assert_eq!(
        value.phase(),
        PrivatePublicationPhase::PreservedStaleEvidence
    );
    assert!(value.stale_artifact().is_some());
    assert_eq!(fs::read(f.path.join("prefs")).unwrap(), b"possibly visible");
}
#[test]
fn partial_write_is_one_shot_and_error_retains_native_file_and_evidence() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    assert!(value
        .write_with(b"complete", |file, bytes| {
            let mut file = file;
            file.write_all(&bytes[..3])?;
            Err(FileError::new(
                ErrorCode::StorageFailed,
                "injected partial write",
            ))
        })
        .is_err());
    assert_eq!(value.phase(), PrivatePublicationPhase::WriteFailed);
    assert!(value
        .stage
        .as_ref()
        .unwrap()
        .identity()
        .matches(value.stage.as_ref().unwrap().as_file())
        .unwrap());
    assert!(value.write_all(b"fresh retry").is_err());
    assert!(matches!(value.publish(), PrivatePublishOutcome::Refused(_)));
    assert!(matches!(
        value.cleanup().unwrap(),
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: true
        }
    ));
    assert_eq!(fs::read(f.path.join(".pending")).unwrap(), b"com");
}
#[test]
fn post_create_preparation_failure_keeps_the_original_custody_not_plain_error() {
    let f = Fixture::new();
    let failed = crate::private_creation::retained::create_with(
        f.parent(),
        OsStr::new("failed"),
        PrivateKind::File,
        PrivateFileUse::Stage,
        |_| {
            Err(FileError::new(
                ErrorCode::StorageFailed,
                "injected postcreate failure",
            ))
        },
    )
    .err()
    .unwrap();
    let held = failed.custody().unwrap();
    assert!(held.as_file().is_some());
    assert!(held.identity().is_some());
    held.verify_retained_parent().unwrap();
    assert!(f.path.join("failed").exists());
    let (error, mut held) = failed.into_parts();
    assert_eq!(error.code, ErrorCode::StorageFailed);
    held.as_mut().unwrap().preserve_stale();
    assert_eq!(
        held.as_ref().unwrap().preservation_state(),
        crate::PrivatePreservationState::StaleArtifactEvidence
    );
    assert!(f.path.join("failed").exists());
    let failed =
        prepare_private_publication_at(f.parent(), OsStr::new("prefs"), OsStr::new("failed"), None)
            .err()
            .unwrap();
    assert!(failed.attempt().is_some());
    assert_eq!(
        failed.attempt().unwrap().phase(),
        PrivatePublicationPhase::PreparationFailed
    );
}
#[test]
fn pre_rename_boundary_unwind_keeps_actual_stage_and_owned_version() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    value.write_all(b"retained stage").unwrap();
    let original = descriptor(value.stage.as_ref().unwrap().as_file());
    let version = value.owned_version.clone().unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| {
        value.publish_with_ops_checked(
            |_, _, _, _, _| panic!("rename must not run"),
            |_| panic!("sync must not run"),
            || panic!("injected pre-rename boundary unwind"),
        )
    }));
    assert!(result.is_err());
    assert_eq!(
        descriptor(value.stage.as_ref().unwrap().as_file()),
        original
    );
    assert_eq!(value.owned_version.as_ref(), Some(&version));
    assert!(value.published_receipt().is_none());
    assert!(!f.path.join("prefs").exists());
    value.preserve_stale();
    let (leaf, id, owned_version) = value.stale_artifact().unwrap();
    assert_eq!(leaf, OsStr::new(".pending"));
    assert_eq!(id, Some(version.identity()));
    assert_eq!(owned_version, Some(&version));
    assert_eq!(
        fs::read(f.path.join(".pending")).unwrap(),
        b"retained stage"
    );
}
#[test]
fn post_rename_sync_unwind_finds_same_fd_installed_before_fallible_work() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    value.write_all(b"visible retained").unwrap();
    let original = descriptor(value.stage.as_ref().unwrap().as_file());
    let version = value.owned_version.clone().unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| {
        value.publish_with_sync(|_| panic!("injected sync unwind"))
    }));
    assert!(result.is_err());
    assert!(value.stage.is_none());
    let published = value.published_receipt().unwrap();
    assert_eq!(descriptor(published.as_file()), original);
    assert_eq!(published.version(), &version);
    assert_eq!(
        value.published.as_ref().unwrap().owned.0.leaf,
        OsStr::new("prefs")
    );
    assert_eq!(
        value.phase(),
        PrivatePublicationPhase::PublishedDurabilityUnconfirmed
    );
    value.preserve_stale();
    let (leaf, id, owned_version) = value.stale_artifact().unwrap();
    assert_eq!(leaf, OsStr::new("prefs"));
    assert_eq!(id, Some(version.identity()));
    assert_eq!(owned_version, Some(&version));
    assert_eq!(fs::read(f.path.join("prefs")).unwrap(), b"visible retained");
}
#[test]
fn preserving_partial_write_keeps_identity_without_fabricating_content_version() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    let identity = value.stage.as_ref().unwrap().identity().clone();
    assert!(value
        .write_with(b"partial", |file, bytes| {
            let mut file = file;
            file.write_all(&bytes[..3])?;
            Err(state_error())
        })
        .is_err());
    assert!(value.owned_version.is_none());
    value.preserve_stale();
    let (leaf, id, version) = value.stale_artifact().unwrap();
    assert_eq!(leaf, OsStr::new(".pending"));
    assert_eq!(id, Some(&identity));
    assert!(version.is_none());
    assert_eq!(fs::read(f.path.join(".pending")).unwrap(), b"par");
}
#[cfg(unix)]
#[test]
fn unix_cleanup_after_namespace_change_preserves_foreign_binding_without_no_owner_claim() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    value.write_all(b"ours").unwrap();
    value.publish();
    fs::write(f.path.join("other"), b"replacement").unwrap();
    fs::rename(f.path.join("other"), f.path.join("prefs")).unwrap();
    assert!(matches!(
        value.cleanup().unwrap(),
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: true
        }
    ));
    assert_eq!(fs::read(f.path.join("prefs")).unwrap(), b"replacement");
}
#[cfg(windows)]
#[test]
fn windows_cleanup_checks_original_owned_version_and_keeps_custody_on_failure() {
    let f = Fixture::new();
    let mut value = prepare(&f);
    value.write_all(b"ours").unwrap();
    value.publish();
    fs::write(f.path.join("prefs"), b"external change").unwrap();
    assert!(value.cleanup().is_err());
    assert!(value.published_receipt().is_some());
    assert_eq!(fs::read(f.path.join("prefs")).unwrap(), b"external change");
}
