//! Poison the production mutation mutex ONLY in a selected child unit-test process.
use super::*;
#[test]
fn cleanup_lock_failure_is_explicit_and_preservation_needs_no_fresh_authority() {
    let full = concat!(module_path!(), "::poisoned_cleanup_child");
    let name = full.split_once("::").unwrap().1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--ignored", "--nocapture"])
        .env("SHELLFOX_FILES_ISOLATED_POISON", name)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated cleanup test failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("running 1 test") && stdout.contains("test result: ok. 1 passed"),
        "isolated child did not execute its exact test: {stdout}"
    );
    // The parent process was never poisoned. No clear_poison or poison-deletion acceptance.
    drop(FileService::mutation_lock().unwrap());
}
#[test]
#[ignore = "Invoked only by the exact isolated subprocess test above."]
fn poisoned_cleanup_child() {
    let full = concat!(module_path!(), "::poisoned_cleanup_child");
    let name = full.split_once("::").unwrap().1;
    assert_eq!(
        std::env::var("SHELLFOX_FILES_ISOLATED_POISON").unwrap(),
        name
    );
    let f = Fixture::new();
    let parent = f.prepared();
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard);
    let weak = Arc::downgrade(&cap);
    let mut owner = cap
        .prepare_private_leaf_publication(OsStr::new("prefs"), OsStr::new("stage"), None)
        .unwrap();
    owner.write_all(b"retained").unwrap();
    cap.revoke().unwrap();
    drop(cap);
    assert!(std::thread::spawn(|| {
        let _guard = FileService::mutation_lock().unwrap();
        panic!("isolated deliberate mutex poison");
    })
    .join()
    .is_err());
    let error = owner.cleanup().unwrap_err();
    assert_eq!(error.code, crate::ErrorCode::StorageFailed);
    assert_eq!(owner.phase(), PrivatePublicationPhase::Ready);
    assert!(weak.upgrade().is_some());
    assert_eq!(
        owner.preserve_stale(),
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: false
        }
    );
    assert_eq!(
        fs::read(f.path.join("prepared/stage")).unwrap(),
        b"retained"
    );
    assert!(owner.cleanup().is_err());
    assert!(FileService::mutation_lock().is_err());
    drop(owner);
    assert!(weak.upgrade().is_none());
}
