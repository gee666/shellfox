use crate::support::{loc, Fixture};
use shellfox_files::*;
use std::fs;

#[test]
fn browse_stat_and_text_keep_utf8_bom_and_crlf() {
    let f = Fixture::new();
    f.write("a.txt", "old\r\n雪\u{feff}");
    f.write("z.txt", "\u{feff}first");
    fs::create_dir(f.root.join("dir")).unwrap();
    let list = f.service.list(&loc(".")).unwrap();
    assert!(list.parent_path.is_none());
    assert_eq!(list.entries[0].kind, EntryKind::Directory);
    assert_eq!(list.entries.len(), 3);
    let stat = f.service.stat(&loc("a.txt")).unwrap();
    assert_eq!(stat.kind, EntryKind::File);
    assert_eq!(stat.size, 11);
    assert_eq!(
        f.service.read_text(&loc("z.txt")).unwrap().text,
        "\u{feff}first"
    );
    let text = f.service.read_text(&loc("a.txt")).unwrap();
    assert_eq!(text.text, "old\r\n雪\u{feff}");
    assert_eq!(text.revision.len(), 64);
    assert_eq!(
        f.service.stat(&loc(".")).unwrap().kind,
        EntryKind::Directory
    );
}

#[test]
fn save_is_atomic_conflicts_are_not_retried_and_stages_are_cleaned() {
    let f = Fixture::new();
    f.write("a.txt", "first");
    let initial = f.service.read_text(&loc("a.txt")).unwrap();
    let save = WriteText {
        path: "a.txt".into(),
        text: "second\r\n雪".into(),
        revision: initial.revision,
    };
    let result = f.service.write_text(&save).unwrap();
    assert_eq!(f.read("a.txt"), save.text.as_bytes());
    assert_eq!(
        result.revision,
        f.service.read_text(&loc("a.txt")).unwrap().revision
    );
    assert_eq!(
        f.service.write_text(&save).unwrap_err().code,
        ErrorCode::FileConflict
    );
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
}

#[test]
fn create_rename_delete_have_explicit_noclobber_and_nonrecursive_semantics() {
    let f = Fixture::new();
    let create = CreateFile {
        path: "new.txt".into(),
        text: "new".into(),
    };
    f.service.create_file(&create).unwrap();
    assert_eq!(
        f.service.create_file(&create).unwrap_err().code,
        ErrorCode::FileConflict
    );
    f.service.create_directory(&loc("folder")).unwrap();
    f.service.create_directory(&loc("folder")).unwrap();
    f.service.create_directory(&loc("nested/a/b")).unwrap();
    assert!(f.root.join("nested/a/b").is_dir());
    assert!(f.service.create_directory(&loc("new.txt/child")).is_err());
    f.service
        .rename(&Rename {
            path: "new.txt".into(),
            destination: "folder/file.txt".into(),
        })
        .unwrap();
    f.write("existing", "original");
    assert_eq!(
        f.service
            .rename(&Rename {
                path: "folder/file.txt".into(),
                destination: "existing".into()
            })
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    assert_eq!(f.read("existing"), b"original");
    f.service
        .rename(&Rename {
            path: "folder".into(),
            destination: "renamed".into(),
        })
        .unwrap();
    assert!(f
        .service
        .delete(&Delete {
            path: "renamed".into(),
            kind: DeleteKind::EmptyDirectory
        })
        .is_err());
    assert_eq!(
        f.service
            .delete(&Delete {
                path: "renamed/file.txt".into(),
                kind: DeleteKind::EmptyDirectory
            })
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    f.service
        .delete(&Delete {
            path: "renamed/file.txt".into(),
            kind: DeleteKind::File,
        })
        .unwrap();
    f.service
        .delete(&Delete {
            path: "renamed".into(),
            kind: DeleteKind::EmptyDirectory,
        })
        .unwrap();
    assert_eq!(
        f.service
            .delete(&Delete {
                path: ".".into(),
                kind: DeleteKind::EmptyDirectory
            })
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
}

#[test]
fn bounded_previews_detect_magic_binary_invalid_utf8_and_large_files() {
    let f = Fixture::new();
    f.write("a.png", b"\x89PNG\r\n\x1a\nanything");
    assert!(
        matches!(f.service.preview(&loc("a.png")).unwrap(), Preview::Image { mime_type, .. } if mime_type == "image/png")
    );
    for bytes in [vec![0, 1], vec![255], vec![b'x'; TEXT_BYTES + 1]] {
        f.write("binary", bytes);
        assert!(matches!(
            f.service.preview(&loc("binary")).unwrap(),
            Preview::Binary { .. }
        ));
        assert_eq!(
            f.service.read_text(&loc("binary")).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
    f.write("large", vec![b'x'; PREVIEW_BYTES + 1]);
    assert_eq!(
        f.service.preview(&loc("large")).unwrap_err().code,
        ErrorCode::Unsupported
    );
    for text in ["a\0b".to_owned(), "x".repeat(TEXT_BYTES + 1)] {
        assert_eq!(
            f.service
                .create_file(&CreateFile {
                    path: "no".into(),
                    text
                })
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
    }
}

#[test]
fn listing_is_bounded() {
    let f = Fixture::new();
    for index in 0..LIST_LIMIT + 2 {
        f.write(&format!("file-{index}"), "");
    }
    let list = f.service.list(&loc(".")).unwrap();
    assert!(list.truncated);
    assert!(list.entries.len() <= LIST_LIMIT);
    assert!(serde_json::to_vec(&list).unwrap().len() <= LIST_BYTES);
}

#[test]
fn strict_requests_reject_extra_fields_and_runtime_validates_revisions() {
    assert!(serde_json::from_str::<Location>(r#"{"path":".","root":"/"}"#).is_err());
    let f = Fixture::new();
    f.write("a", "old");
    assert_eq!(
        f.service
            .write_text(&WriteText {
                path: "a".into(),
                text: "new".into(),
                revision: "x".repeat(64)
            })
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert_eq!(
        f.service.read_text(&loc("missing")).unwrap_err().code,
        ErrorCode::NotFound
    );
}
