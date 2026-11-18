use super::*;
#[path = "cap_private_cleanup_lock_tests.rs"]
mod cleanup_lock;
#[path = "cap_private_child_tests.rs"]
mod private_child;
#[path = "private_root_check_candidate_tests.rs"]
mod private_root_checks;
use crate::{
    PrivateCleanupDisposition, PrivatePublicationPhase, PrivatePublishOutcome,
    PRIVATE_METADATA_MAX_BYTES,
};
use std::{ffi::OsStr, sync::atomic::AtomicUsize};
fn root(
    f: &Fixture,
    parent: &CreatedPrivate,
    guard: Arc<dyn NativeCommitFence>,
) -> Arc<FileService> {
    f.issuer()
        .native_child_issuer(PreparedPrivateDirectoryRef::Created(parent), guard)
        .unwrap()
        .parent_root()
}
#[derive(Default)]
struct SampleFence {
    calls: AtomicUsize,
    deny: AtomicUsize,
    collision: std::sync::Mutex<Option<(usize, PathBuf)>>,
}
impl NativeCommitFence for SampleFence {
    fn check(&self) -> Result<()> {
        let sample = self.calls.fetch_add(1, Ordering::AcqRel) + 1;
        if self.deny.load(Ordering::Acquire) == sample {
            return Err(FileError::denied());
        }
        // Deterministic owned-fixture interference AFTER prepublish checks, BEFORE native rename.
        // It is a kernel-collision test, not a production fence implementation or cleanup grant.
        if let Some((at, path)) = self.collision.lock().unwrap().as_ref() {
            if *at == sample {
                fs::create_dir(path).unwrap();
            }
        }
        Ok(())
    }
}
#[test]
fn private_leaf_metadata_uses_held_scope_not_an_existing_diagnostic_label() {
    let f = Fixture::new();
    let parent = f.prepared();
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard.clone());
    let decoy = f.path.join("decoy");
    fs::create_dir(&decoy).unwrap();
    fs::write(decoy.join("prefs"), b"decoy").unwrap();
    let cap = Arc::new(
        cap.derive_anchored_relative(
            cap.retained_anchored_directory().unwrap(),
            decoy.to_str().unwrap().into(),
            guard,
        )
        .unwrap(),
    );
    let mut owner = cap
        .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("stage"), None)
        .unwrap();
    owner.write_all(b"native").unwrap();
    assert!(owner.write_all(b"replay").is_err());
    assert!(matches!(
        owner.publish(),
        PrivatePublishOutcome::Published(_)
    ));
    let version = owner.published_receipt().unwrap().version().clone();
    assert_eq!(
        owner.published_receipt().unwrap().identity(),
        version.identity()
    );
    drop(owner);
    assert_eq!(
        cap.read_private_leaf(OsStr::new("prefs"), 6).unwrap().bytes,
        b"native"
    );
    assert!(cap.read_private_leaf(OsStr::new("prefs"), 5).is_err());
    let mut replacement = cap
        .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("next"), Some(&version))
        .unwrap();
    replacement.write_all(b"updated").unwrap();
    assert!(matches!(
        replacement.publish(),
        PrivatePublishOutcome::Published(_)
    ));
    drop(replacement);
    assert_eq!(
        cap.read_private_leaf(OsStr::new("prefs"), 64)
            .unwrap()
            .bytes,
        b"updated"
    );
    assert_eq!(fs::read(decoy.join("prefs")).unwrap(), b"decoy");
    assert!(!decoy.join("stage").exists());
    cap.revoke().unwrap();
    assert!(cap.read_private_leaf(OsStr::new("prefs"), 64).is_err());
    assert!(cap
        .prepare_private_leaf_publication(OsStr::new("denied"), OsStr::new("absent"), None)
        .is_err());
    assert!(!f.path.join("prepared/absent").exists());
}
#[test]
fn metadata_api_rejects_path_account_and_nonleaf_inputs_without_creating_artifacts() {
    let f = Fixture::new();
    let parent = f.prepared();
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard);
    for bad in [
        Arc::new(FileService::approved_root(&f.path, Access::ReadWrite).unwrap()),
        f.issuer(),
        Arc::new(FileService::approved_full_account()),
    ] {
        assert!(bad.read_private_leaf(OsStr::new("prefs"), 64).is_err());
        assert!(bad
            .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("forbidden"), None)
            .is_err());
    }
    for leaf in [
        "../outside",
        "sub/leaf",
        "C:leaf",
        "\\leaf",
        "/leaf",
        ".",
        "..",
    ] {
        assert!(cap.read_private_leaf(OsStr::new(leaf), 64).is_err());
        assert!(cap
            .prepare_private_leaf_publication(OsStr::new(leaf), OsStr::new("stage"), None)
            .is_err());
    }
    assert_eq!(fs::read_dir(f.path.join("prepared")).unwrap().count(), 0);
}
#[test]
fn write_is_bounded_one_shot_and_cleanup_remains_owned_after_revocation() {
    let f = Fixture::new();
    let parent = f.prepared();
    let (_lost, keeper, guard) = fence();
    let custody = Arc::downgrade(&keeper);
    let cap = root(&f, &parent, guard);
    let weak = Arc::downgrade(&cap);
    let mut owner = cap
        .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("stage"), None)
        .unwrap();
    assert!(owner
        .write_all(&vec![0; PRIVATE_METADATA_MAX_BYTES + 1])
        .is_err());
    assert!(owner.write_all(b"retry").is_err());
    assert_eq!(owner.phase(), PrivatePublicationPhase::NotPublished);
    cap.revoke().unwrap();
    assert!(owner.write_all(b"denied").is_err());
    assert!(matches!(owner.publish(), PrivatePublishOutcome::Refused(_)));
    drop(cap);
    drop(keeper);
    assert!(weak.upgrade().is_some());
    assert!(custody.upgrade().is_some());
    let disposition = owner.cleanup().unwrap();
    #[cfg(unix)]
    assert_eq!(
        disposition,
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: true
        }
    );
    #[cfg(windows)]
    assert_eq!(disposition, PrivateCleanupDisposition::DeletePending);
    assert_eq!(
        owner.preserve_stale(),
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: false
        }
    );
    drop(owner);
    assert!(weak.upgrade().is_none());
    assert!(custody.upgrade().is_none());
}
#[test]
fn final_same_cap_sample_denies_write_and_commit_without_watchers() {
    let f = Fixture::new();
    let parent = f.prepared();
    let guard = Arc::new(SampleFence::default());
    let cap = root(&f, &parent, guard.clone());
    let mut failed = cap
        .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("write-stage"), None)
        .unwrap();
    guard
        .deny
        .store(guard.calls.load(Ordering::Acquire) + 2, Ordering::Release);
    assert!(failed.write_all(b"partial-not-allowed").is_err());
    assert_eq!(failed.phase(), PrivatePublicationPhase::WriteFailed);
    assert!(failed.write_all(b"retry").is_err());
    let mut ready = cap
        .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("commit-stage"), None)
        .unwrap();
    ready.write_all(b"prepared").unwrap();
    guard
        .deny
        .store(guard.calls.load(Ordering::Acquire) + 2, Ordering::Release);
    assert!(matches!(
        ready.publish(),
        PrivatePublishOutcome::NotPublished(_)
    ));
    assert_eq!(ready.phase(), PrivatePublicationPhase::NotPublished);
    assert!(ready.published_receipt().is_none());
    assert!(!f.path.join("prepared/prefs").exists());
    let mut late = cap
        .prepare_private_leaf_publication(OsStr::new("late"), OsStr::new("late-stage"), None)
        .unwrap();
    late.write_all(b"held").unwrap();
    // Deny at the literal native syscall boundary, AFTER the pre-rename sample.
    guard
        .deny
        .store(guard.calls.load(Ordering::Acquire) + 3, Ordering::Release);
    assert!(matches!(
        late.publish(),
        PrivatePublishOutcome::Uncertain(_)
    ));
    assert!(!f.path.join("prepared/late").exists());
    cap.revoke().unwrap();
    assert_eq!(
        late.cleanup().unwrap(),
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: true
        }
    );
    late.preserve_stale();
    assert!(failed.cleanup().is_ok());
    assert!(ready.cleanup().is_ok());
    failed.preserve_stale();
    ready.preserve_stale();
}
#[test]
fn preparation_failure_retains_same_cap_without_taking_ownership_of_collision() {
    let f = Fixture::new();
    let parent = f.prepared();
    text(parent.as_file(), "collision", "other");
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard);
    let weak = Arc::downgrade(&cap);
    let failure = cap
        .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("collision"), None)
        .err()
        .unwrap();
    assert!(failure.attempt().is_some());
    assert_eq!(
        failure.attempt().unwrap().phase(),
        PrivatePublicationPhase::PreparationFailed
    );
    cap.revoke().unwrap();
    drop(cap);
    let (_error, owner) = failure.into_parts();
    let mut owner = owner.unwrap();
    assert!(weak.upgrade().is_some());
    assert!(owner.cleanup().is_ok());
    owner.preserve_stale();
    assert_eq!(
        fs::read(f.path.join("prepared/collision")).unwrap(),
        b"other"
    );
    drop(owner);
    assert!(weak.upgrade().is_none());
}
#[test]
fn uncertain_native_rename_keeps_custody_and_never_replays_even_after_revocation() {
    let f = Fixture::new();
    let parent = f.prepared();
    let guard = Arc::new(SampleFence::default());
    let cap = root(&f, &parent, guard.clone());
    let weak = Arc::downgrade(&cap);
    let mut owner = cap
        .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("stage"), None)
        .unwrap();
    owner.write_all(b"held").unwrap();
    *guard.collision.lock().unwrap() = Some((
        guard.calls.load(Ordering::Acquire) + 2,
        f.path.join("prepared/prefs"),
    ));
    assert!(matches!(
        owner.publish(),
        PrivatePublishOutcome::Uncertain(_)
    ));
    assert_eq!(owner.phase(), PrivatePublicationPhase::Uncertain);
    assert!(owner.published_receipt().is_none());
    assert!(matches!(owner.publish(), PrivatePublishOutcome::Refused(_)));
    cap.revoke().unwrap();
    drop(cap);
    assert_eq!(
        owner.cleanup().unwrap(),
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: true
        }
    );
    assert!(weak.upgrade().is_some());
    owner.preserve_stale();
    assert_eq!(owner.stale_artifact().unwrap().0, OsStr::new("stage"));
    assert!(f.path.join("prepared/stage").exists());
    drop(owner);
    assert!(weak.upgrade().is_none());
}
