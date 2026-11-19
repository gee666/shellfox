use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        fs::create_dir_all(&tmp).unwrap();
        let path = tmp
            .canonicalize()
            .unwrap()
            .join(format!("held-editor-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn retained_file_rejects_same_byte_replacement_after_successive_saves() {
    let scratch = Scratch::new();
    let path = scratch.0.join("file.txt");
    fs::write(&path, "first\n").unwrap();
    let service = Arc::new(FileService::local_desktop(Access::ReadWrite));
    let backend = EditorBackend::new(service.clone(), scratch.0.join("journal")).unwrap();
    let (mut editor, ReadResult::Text { revision, .. }) = HeldEditor::open(
        &service,
        &Location {
            path: path.to_str().unwrap().into(),
        },
    )
    .unwrap() else {
        panic!("expected text")
    };
    let mut input = WriteText {
        path: path.to_str().unwrap().into(),
        revision,
        text: "\u{feff}déjà\r\n".into(),
    };
    input.revision = editor.write(&backend, &input).unwrap().revision;
    assert_eq!(fs::read(&path).unwrap(), input.text.as_bytes());
    input.text = "second save\n".into();
    input.revision = editor.write(&backend, &input).unwrap().revision;
    fs::rename(&path, scratch.0.join("replaced.txt")).unwrap();
    fs::write(&path, &input.text).unwrap();
    input.text = "must not publish\n".into();
    assert_eq!(
        editor.write(&backend, &input).unwrap_err().code,
        ErrorCode::FileConflict
    );
    assert_eq!(fs::read(&path).unwrap(), b"second save\n");
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 3);
}

#[test]
fn retained_file_rejects_external_edit_and_parent_replacement() {
    let scratch = Scratch::new();
    let parent = scratch.0.join("owned");
    fs::create_dir(&parent).unwrap();
    let path = parent.join("file.txt");
    fs::write(&path, "original\n").unwrap();
    let service = Arc::new(FileService::local_desktop(Access::ReadWrite));
    let backend = EditorBackend::new(service.clone(), scratch.0.join("journal")).unwrap();
    let location = Location {
        path: path.to_str().unwrap().into(),
    };
    let open = || {
        let (editor, ReadResult::Text { revision, .. }) =
            HeldEditor::open(&service, &location).unwrap()
        else {
            panic!("expected text")
        };
        (
            editor,
            WriteText {
                path: location.path.clone(),
                revision,
                text: "must not publish\n".into(),
            },
        )
    };
    let (mut editor, input) = open();
    fs::write(&path, "external edit\n").unwrap();
    assert_eq!(
        editor.write(&backend, &input).unwrap_err().code,
        ErrorCode::FileConflict
    );
    assert_eq!(fs::read(&path).unwrap(), b"external edit\n");
    let (mut editor, input) = open();
    let moved = scratch.0.join("moved");
    fs::rename(&parent, &moved).unwrap();
    fs::create_dir(&parent).unwrap();
    fs::hard_link(moved.join("file.txt"), &path).unwrap();
    assert_eq!(
        editor.write(&backend, &input).unwrap_err().code,
        ErrorCode::FileConflict
    );
    assert_eq!(fs::read(&path).unwrap(), b"external edit\n");
}
