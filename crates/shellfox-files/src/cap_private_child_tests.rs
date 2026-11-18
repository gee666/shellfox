use super::*;
#[cfg(windows)]
fn privacy(path: &Path) -> Vec<u8> {
    crate::private_test_windows::security(path)
}
#[cfg(unix)]
fn privacy(path: &Path) -> Vec<u8> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).unwrap().mode().to_le_bytes().to_vec()
}
fn expose(path: &Path) {
    #[cfg(windows)]
    crate::private_test_windows::unprotect(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
#[test]
fn single_leaf_child_preserves_native_scope_identity_ledger_and_metadata_namespace() {
    let f = Fixture::new();
    let parent = f.prepared();
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard.clone());
    let decoy = f.path.join("decoy");
    fs::create_dir(&decoy).unwrap();
    let cap = Arc::new(
        cap.derive_anchored_relative(
            cap.retained_anchored_directory().unwrap(),
            decoy.to_str().unwrap().into(),
            guard,
        )
        .unwrap(),
    );
    let child = cap
        .open_or_create_private_child(OsStr::new("remote-v1"))
        .unwrap();
    assert!(child.root_path().is_none());
    assert_eq!(child.capability_kind(), CapabilityKind::ApprovedRoot);
    assert!(Arc::ptr_eq(&cap.plan_creations, &child.plan_creations));
    assert_eq!(
        child.native_commit_fences.len(),
        cap.native_commit_fences.len()
    );
    for (a, b) in cap
        .native_commit_fences
        .iter()
        .zip(&child.native_commit_fences)
    {
        assert!(Arc::ptr_eq(a, b));
    }
    let identity =
        crate::HandleIdentity::from_directory(&child.retained_anchored_directory().unwrap())
            .unwrap();
    let again = cap
        .open_or_create_private_child(OsStr::new("remote-v1"))
        .unwrap();
    assert_eq!(
        identity,
        crate::HandleIdentity::from_directory(&again.retained_anchored_directory().unwrap())
            .unwrap()
    );
    assert_eq!(
        cap.directory_preparation_owner()
            .retention()
            .unwrap()
            .created,
        1
    );
    let mut owner = child
        .prepare_private_leaf_publication(
            OsStr::new("approvals.json"),
            OsStr::new("write-pending"),
            None,
        )
        .unwrap();
    owner.write_all(b"private").unwrap();
    assert!(matches!(
        owner.publish(),
        PrivatePublishOutcome::Published(_)
    ));
    drop(owner);
    assert_eq!(
        again
            .read_private_leaf(OsStr::new("approvals.json"), 64)
            .unwrap()
            .bytes,
        b"private"
    );
    assert!(!decoy.join("remote-v1").exists());
    assert!(!f.path.join("prepared/approvals.json").exists());
    cap.directory_preparation_owner().verify_retained().unwrap();
    cap.revoke().unwrap();
    assert!(child
        .read_private_leaf(OsStr::new("approvals.json"), 64)
        .is_err());
    assert!(again
        .open_or_create_private_child(OsStr::new("denied"))
        .is_err());
    assert!(!f.path.join("prepared/remote-v1/denied").exists());
}
#[test]
fn existing_private_child_is_explicitly_reopened_without_recording_created_ownership() {
    let f = Fixture::new();
    let parent = f.prepared();
    let existing = create_private_directory_at(parent.as_file(), OsStr::new("existing")).unwrap();
    let identity = existing.identity().clone();
    drop(existing);
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard);
    let child = cap
        .open_or_create_private_child(OsStr::new("existing"))
        .unwrap();
    assert_eq!(
        identity,
        crate::HandleIdentity::from_directory(&child.retained_anchored_directory().unwrap())
            .unwrap()
    );
    assert_eq!(
        cap.directory_preparation_owner()
            .retention()
            .unwrap()
            .created,
        0
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let path = f.path.join("prepared/existing");
        let mut options = fs::OpenOptions::new();
        options
            .access_mode(0x10000)
            .share_mode(7)
            .custom_flags(0x02000000 | 0x00200000);
        assert_eq!(options.open(&path).unwrap_err().raw_os_error(), Some(32));
        drop(child);
        assert!(options.open(path).is_ok());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            fs::metadata(f.path.join("prepared/existing"))
                .unwrap()
                .mode()
                & 0o7777,
            0o700
        );
    }
}
#[test]
fn invalid_scope_access_loss_and_nonleaf_inputs_make_no_children() {
    let f = Fixture::new();
    let parent = f.prepared();
    let (lost, _keeper, guard) = fence();
    let mut cap = root(&f, &parent, guard);
    for bad in [
        "",
        ".",
        "..",
        "remote-v1/child",
        "../outside",
        "/absolute",
        "C:leaf",
        "\\leaf",
    ] {
        assert!(cap.open_or_create_private_child(OsStr::new(bad)).is_err());
    }
    for bad in [
        f.issuer(),
        Arc::new(FileService::approved_full_account()),
        Arc::new(FileService::approved_root(&f.path, Access::ReadWrite).unwrap()),
    ] {
        assert!(bad
            .open_or_create_private_child(OsStr::new("denied"))
            .is_err());
    }
    Arc::get_mut(&mut cap).unwrap().access = Access::ReadOnly;
    assert!(cap
        .open_or_create_private_child(OsStr::new("readonly"))
        .is_err());
    Arc::get_mut(&mut cap).unwrap().access = Access::ReadWrite;
    lost.store(true, Ordering::Release);
    assert!(cap
        .open_or_create_private_child(OsStr::new("lost"))
        .is_err());
    assert_eq!(
        cap.directory_preparation_owner()
            .retention()
            .unwrap()
            .created,
        0
    );
    assert_eq!(fs::read_dir(f.path.join("prepared")).unwrap().count(), 0);
}
#[test]
fn parent_and_existing_child_privacy_are_verify_only_and_never_repaired() {
    let f = Fixture::new();
    let parent = f.prepared();
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard);
    let child = create_private_directory_at(parent.as_file(), OsStr::new("existing")).unwrap();
    let path = f.path.join("prepared/existing");
    expose(&path);
    let before = privacy(&path);
    assert!(cap
        .open_or_create_private_child(OsStr::new("existing"))
        .is_err());
    assert_eq!(privacy(&path), before);
    drop(child);
    text(parent.as_file(), "file", "keep");
    assert!(cap
        .open_or_create_private_child(OsStr::new("file"))
        .is_err());
    assert_eq!(fs::read(f.path.join("prepared/file")).unwrap(), b"keep");
    let path = f.path.join("prepared");
    expose(&path);
    let before = privacy(&path);
    assert!(cap
        .open_or_create_private_child(OsStr::new("absent"))
        .is_err());
    assert_eq!(privacy(&path), before);
    assert!(!path.join("absent").exists());
    assert_eq!(
        cap.directory_preparation_owner()
            .retention()
            .unwrap()
            .created,
        0
    );
}
#[test]
fn link_collision_never_follows_or_repairs_a_different_private_directory() {
    let f = Fixture::new();
    let parent = f.prepared();
    let outside = create_private_directory_at(f.parent(), OsStr::new("outside")).unwrap();
    let target = f.path.join("outside");
    let link = f.path.join("prepared/link");
    #[cfg(windows)]
    crate::private_test_junction::create(&target, &link);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard);
    assert!(cap
        .open_or_create_private_child(OsStr::new("link"))
        .is_err());
    assert!(fs::symlink_metadata(&link).is_ok());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    assert_eq!(
        cap.directory_preparation_owner()
            .retention()
            .unwrap()
            .created,
        0
    );
    drop(outside);
}
#[test]
fn retained_helper_completes_with_borrowed_admission_mutation_guard() {
    let f = Fixture::new();
    let parent = f.prepared();
    let (_lost, _keeper, guard) = fence();
    let cap = root(&f, &parent, guard);
    let mut admission = cap.acquire_directory_admission().unwrap();
    assert!(matches!(
        crate::service::MUTATIONS.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    let held = cap.retained_anchored_directory().unwrap();
    // Admission already owns the real global guard. The helper must not reacquire it.
    admission.mark_effect_possible();
    let child = crate::directory_plan::create::create_private_child_retained(
        &cap,
        &held,
        OsStr::new("held-lock"),
        admission.mutation(),
    )
    .unwrap();
    assert!(crate::HandleIdentity::from_directory(&child).is_ok());
    assert_eq!(
        cap.directory_preparation_owner()
            .retention()
            .unwrap()
            .created,
        1
    );
    drop(child);
    admission.finish(Ok(())).unwrap();
    assert!(cap
        .open_or_create_private_child(OsStr::new("public-completes"))
        .is_ok());
    assert_eq!(
        cap.directory_preparation_owner()
            .retention()
            .unwrap()
            .created,
        2
    );
}
struct AfterCreate {
    path: PathBuf,
    lost: AtomicBool,
}
impl NativeCommitFence for AfterCreate {
    fn check(&self) -> Result<()> {
        if self.path.is_dir() {
            self.lost.store(true, Ordering::Release);
        }
        if self.lost.load(Ordering::Acquire) {
            Err(FileError::denied())
        } else {
            Ok(())
        }
    }
}
#[test]
fn postcreate_loss_retains_original_bounded_keeper_in_typed_failure_without_unlink() {
    let f = Fixture::new();
    let parent = f.prepared();
    let guard = Arc::new(AfterCreate {
        path: f.path.join("prepared/remote-v1"),
        lost: AtomicBool::new(false),
    });
    let cap = root(&f, &parent, guard);
    let weak = Arc::downgrade(&cap);
    let mut failure = cap
        .open_or_create_private_child(OsStr::new("remote-v1"))
        .err()
        .unwrap();
    assert_eq!(failure.retention().unwrap().created, 1);
    failure.verify_retained().unwrap();
    drop(cap);
    assert!(weak.upgrade().is_some());
    assert!(f.path.join("prepared/remote-v1").is_dir());
    assert_eq!(
        failure.preserve_stale().unwrap(),
        crate::PrivatePreservationState::StaleArtifactEvidence
    );
    assert_eq!(failure.retention().unwrap().stale_evidence, 1);
    assert!(failure.verify_retained().is_ok());
    assert!(f.path.join("prepared/remote-v1").is_dir());
    drop(failure);
    assert!(weak.upgrade().is_none());
}
