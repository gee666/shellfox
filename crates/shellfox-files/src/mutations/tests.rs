use super::*;
use std::{fs, path::Path};

struct Fixture {
    root: PathBuf,
    service: FileService,
}
impl Fixture {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        let root = root.canonicalize().unwrap();
        let text = root
            .to_str()
            .unwrap()
            .trim_start_matches("\\\\?\\")
            .to_owned();
        let root = Path::new(&text).join(format!("rust-files-race-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self {
            service: FileService::approved_root(&root, Access::ReadWrite).unwrap(),
            root,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(std::mem::replace(
            &mut self.service,
            FileService::local_desktop(Access::ReadOnly),
        ));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn external_edit_before_commit_conflicts_and_cleans_stage() {
    let f = Fixture::new();
    fs::write(f.root.join("a"), "first").unwrap();
    let result = f
        .service
        .save("a", "ours", Some(&content::revision(b"first")), || {
            fs::write(f.root.join("a"), "other").unwrap();
        });
    assert_eq!(result.unwrap_err().code, ErrorCode::FileConflict);
    assert_eq!(fs::read(f.root.join("a")).unwrap(), b"other");
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
}

#[test]
fn competing_create_never_overwrites() {
    let f = Fixture::new();
    let result = f.service.save("new", "ours", None, || {
        fs::write(f.root.join("new"), "winner").unwrap();
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::FileConflict);
    assert_eq!(fs::read(f.root.join("new")).unwrap(), b"winner");
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
}

#[test]
fn replaced_stage_is_not_committed_or_deleted_as_owned() {
    let f = Fixture::new();
    let mut replacement = PathBuf::new();
    let result = f.service.save("new", "ours", None, || {
        let stage = fs::read_dir(&f.root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(&stage).unwrap();
        fs::write(&stage, "not-ours").unwrap();
        replacement = stage;
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::FileConflict);
    assert!(!f.root.join("new").exists());
    assert_eq!(fs::read(&replacement).unwrap(), b"not-ours");
}

#[test]
fn in_place_stage_tampering_is_not_published() {
    let f = Fixture::new();
    let result = f.service.save("new", "ours", None, || {
        let stage = fs::read_dir(&f.root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(stage, "evil").unwrap();
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::FileConflict);
    assert!(!f.root.join("new").exists());
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
}

#[test]
fn independent_services_serialize_revision_checked_saves() {
    let f = Fixture::new();
    fs::write(f.root.join("a"), "old").unwrap();
    let second = FileService::approved_root(&f.root, Access::ReadWrite).unwrap();
    let revision = content::revision(b"old");
    let request = |text: &str| WriteText {
        path: "a".into(),
        text: text.into(),
        revision: revision.clone(),
    };
    let (first, second) = std::thread::scope(|scope| {
        let a = scope.spawn(|| f.service.write_text(&request("first")));
        let b = scope.spawn(|| second.write_text(&request("second")));
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "first: {first:?}; second: {second:?}"
    );
    assert_eq!(
        first.err().or_else(|| second.err()).unwrap().code,
        ErrorCode::FileConflict
    );
}

#[cfg(unix)]
#[test]
fn parent_replacement_during_save_never_redirects_commit() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("dir")).unwrap();
    let result = f.service.save("dir/new", "ours", None, || {
        fs::rename(f.root.join("dir"), f.root.join("moved")).unwrap();
        fs::create_dir(f.root.join("dir")).unwrap();
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::FileConflict);
    assert!(!f.root.join("dir/new").exists());
    assert_eq!(fs::read_dir(f.root.join("moved")).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn a_rename_refused_by_another_handle_leaves_no_stage() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    fs::write(f.root.join("t.txt"), "old").unwrap();
    let revision = content::revision(b"old");
    // Another program holds the file open without delete sharing, so the rename fails.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(f.root.join("t.txt"))
        .unwrap();
    let result = f.service.write_text(&WriteText {
        path: "t.txt".into(),
        text: "new".into(),
        revision,
    });
    assert!(result.is_err());
    drop(held);
    let names: Vec<_> = fs::read_dir(&f.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["t.txt"]);
    assert_eq!(fs::read(f.root.join("t.txt")).unwrap(), b"old");
}
