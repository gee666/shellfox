use super::*;
use std::{fs, io::Write, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let root = parent.join(format!("cas-snapshot-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn file(&self, name: &str) -> File {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(self.0.join(name))
            .unwrap();
        file.write_all(b"identical content").unwrap();
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0)).unwrap();
        File::from_std(file)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn snapshot_captures_actual_handle_identity_and_clone_is_same_version() {
    let fixture = Fixture::new();
    let file = fixture.file("a");
    let cloned = file.try_clone().unwrap();
    let first = snapshot(&file).unwrap();
    let second = snapshot(&cloned).unwrap();
    assert_eq!(
        first.identity,
        crate::HandleIdentity::from_cap_file(&file).unwrap()
    );
    assert!(same_version(&first, &second));
    assert!(same_held_file(&file, &cloned).unwrap());
}
#[test]
fn equal_content_revision_is_not_object_identity() {
    let fixture = Fixture::new();
    let mut a = fixture.file("a");
    let mut b = fixture.file("b");
    let (bytes_a, snapshot_a) = read(&mut a, TEXT_BYTES).unwrap();
    let (bytes_b, snapshot_b) = read(&mut b, TEXT_BYTES).unwrap();
    assert_eq!(revision(&bytes_a), revision(&bytes_b));
    assert_ne!(snapshot_a.identity, snapshot_b.identity);
    assert!(!same_version(&snapshot_a, &snapshot_b));
    assert!(!same_held_file(&a, &b).unwrap());
}
