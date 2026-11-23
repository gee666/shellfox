use super::*;
use crate::options::Options;
use shellfox_files::{EditorCancellation, Location, WriteText};
use std::{ffi::OsString, path::PathBuf};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::current_dir().unwrap().join("tmp").join(format!(
            "editor-v2-composition-{}",
            shellfox_core::SessionId::new()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn options(&self) -> Options {
        Options::parse([
            OsString::from("--fixture"),
            OsString::from("--data-dir"),
            self.0.join("host").into_os_string(),
        ])
        .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn editor_admission_matches_review_state_and_keeps_host_namespace_untouched() {
    let scratch = Scratch::new();
    let options = scratch.options();
    let mut frontend = FrontendDirectories::plan(&options).unwrap();
    frontend.prepare().unwrap();
    assert!(matches!(prepare(&mut frontend), EditorOutcome::Ready(_)));
    assert!(!options.data_dir.exists());
}
#[test]
fn production_composition_saves_with_revision_checks_and_no_host_writes() {
    let scratch = Scratch::new();
    let options = scratch.options();
    let user = scratch.0.join("document.txt");
    std::fs::write(&user, b"old").unwrap();
    let mut frontend = FrontendDirectories::plan(&options).unwrap();
    frontend.prepare().unwrap();
    // Exact production composition method, not a test authority or metadata-root regrant.
    let backend = frontend.prepare_editor().unwrap();
    let same = frontend.prepare_editor().unwrap();
    assert!(Arc::ptr_eq(&backend, &same));
    let session = backend.session();
    let file = session
        .read(
            &Location {
                path: user.to_str().unwrap().into(),
            },
            &EditorCancellation::default(),
        )
        .unwrap();
    session
        .write(
            &WriteText {
                path: file.path,
                revision: file.revision,
                text: "new".into(),
            },
            &EditorCancellation::default(),
        )
        .unwrap();
    assert_eq!(std::fs::read(user).unwrap(), b"new");
    assert!(!options.data_dir.exists());
    assert!(!options.desktop_dir.join("file-editor").exists());
    drop(session);
    drop(same);
    drop(backend);
    drop(frontend.preserve());
}
