use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
};
struct Fixture {
    root: PathBuf,
    session: Arc<EditorSession>,
}
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let base = base.to_str().unwrap().trim_start_matches("\\\\?\\");
        let root = Path::new(base).join(format!("rust-editor-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("user")).unwrap();
        let capability =
            Arc::new(FileService::approved_root(root.join("user"), Access::ReadWrite).unwrap());
        let mut registry = TransferRegistry::new(TransferLimits::default()).unwrap();
        registry.initialize_journal(root.join("metadata")).unwrap();
        Self {
            root,
            session: Arc::new(EditorSession::new(capability, Arc::new(Mutex::new(registry)))),
        }
    }
    fn location(&self) -> Location {
        Location {
            path: "document.txt".into(),
        }
    }
    fn path(&self) -> PathBuf {
        self.root.join("user/document.txt")
    }
    fn clean(&self) {
        assert_eq!(self.session.registry.lock().unwrap().usage().active, 0);
        assert_eq!(fs::read_dir(self.root.join("user")).unwrap().count(), 1);
        let journal: serde_json::Value = serde_json::from_slice(
            &fs::read(self.root.join("metadata/file-transfer-stages-v2.json")).unwrap(),
        )
        .unwrap();
        assert!(journal["entries"].as_array().unwrap().is_empty());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let empty = EditorSession::new(
            Arc::new(FileService::local_desktop(Access::ReadOnly)),
            Arc::new(Mutex::new(
                TransferRegistry::new(TransferLimits::default()).unwrap(),
            )),
        );
        drop(std::mem::replace(&mut self.session, Arc::new(empty)));
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn exact_eight_mib_editor_round_trip_preserves_bom_crlf_and_split_utf8() {
    let f = Fixture::new();
    let mut bytes = vec![b'x'; TRANSFER_TEXT_BYTES as usize];
    bytes[..3].copy_from_slice(b"\xef\xbb\xbf");
    bytes[CHUNK_BYTES - 1..CHUNK_BYTES + 2].copy_from_slice("雪".as_bytes());
    let end = bytes.len();
    bytes[end - 2..].copy_from_slice(b"\r\n");
    fs::write(f.path(), &bytes).unwrap();
    assert_eq!(
        f.session
            .capability
            .read_text(&f.location())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    let file = f
        .session
        .read(&f.location(), &EditorCancellation::default())
        .unwrap();
    assert_eq!(file.text.as_bytes(), bytes);
    let mut text = file.text;
    text.replace_range(CHUNK_BYTES - 1..CHUNK_BYTES + 2, "雨");
    let result = f
        .session
        .write(
            &WriteText {
                path: file.path,
                revision: file.revision,
                text: text.clone(),
            },
            &EditorCancellation::default(),
        )
        .unwrap();
    assert_eq!(result.size, TRANSFER_TEXT_BYTES);
    assert_eq!(fs::read(f.path()).unwrap(), text.as_bytes());
    assert!(text.starts_with('\u{feff}'));
    assert!(text.ends_with("\r\n"));
    f.clean();
}
#[test]
fn one_app_backend_shares_a_stable_journal_and_separate_window_sessions_conflict_safely() {
    let f = Fixture::new();
    fs::write(f.path(), "old").unwrap();
    let directory = f.root.join("desktop/file-editor");
    let backend = Arc::new(EditorBackend::new(f.session.capability.clone(), &directory).unwrap());
    assert!(EditorBackend::new(f.session.capability.clone(), &directory).is_err());
    let first = backend.session();
    let second = backend.session();
    let a = first
        .read(&f.location(), &EditorCancellation::default())
        .unwrap();
    let b = second
        .read(&f.location(), &EditorCancellation::default())
        .unwrap();
    first
        .write(
            &WriteText {
                path: a.path,
                revision: a.revision,
                text: "first window".into(),
            },
            &EditorCancellation::default(),
        )
        .unwrap();
    assert_eq!(
        second
            .write(
                &WriteText {
                    path: b.path,
                    revision: b.revision,
                    text: "stale second window".into()
                },
                &EditorCancellation::default()
            )
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    assert_eq!(fs::read(f.path()).unwrap(), b"first window");
    drop(first);
    drop(second);
    drop(backend);
    let reopened = EditorBackend::new(f.session.capability.clone(), &directory).unwrap();
    assert_eq!(
        reopened
            .session()
            .read(&f.location(), &EditorCancellation::default())
            .unwrap()
            .text,
        "first window"
    );
    f.clean();
}
#[test]
fn editor_oversize_is_rejected_without_widening_inline_contracts_or_creating_stages() {
    let f = Fixture::new();
    fs::write(f.path(), vec![b'x'; TRANSFER_TEXT_BYTES as usize + 1]).unwrap();
    assert_eq!(
        f.session
            .read(&f.location(), &EditorCancellation::default())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    fs::write(f.path(), "old").unwrap();
    let file = f
        .session
        .read(&f.location(), &EditorCancellation::default())
        .unwrap();
    assert_eq!(
        f.session
            .write(
                &WriteText {
                    path: file.path,
                    revision: file.revision,
                    text: "x".repeat(TRANSFER_TEXT_BYTES as usize + 1)
                },
                &EditorCancellation::default()
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert_eq!(fs::read(f.path()).unwrap(), b"old");
    assert_eq!(TEXT_BYTES, 128 * 1024);
    f.clean();
}
#[test]
fn document_read_uses_content_kind_and_chunked_limits() {
    let f = Fixture::new();
    let cancel = EditorCancellation::default();
    let read = || f.session.read_document(&f.location(), &cancel);
    fs::write(f.path(), [0xff, 0x00, 0x01]).unwrap();
    assert!(matches!(
        read().unwrap(),
        ReadResult::Binary { size: 3, .. }
    ));
    f.clean();

    let png = b"\x89PNG\r\n\x1a\n";
    fs::write(f.path(), png).unwrap();
    assert!(
        matches!(read().unwrap(), ReadResult::Image { mime_type, bytes, .. } if mime_type == "image/png" && bytes == png)
    );
    f.clean();

    let beyond_inline = "雪\r\n".repeat(TEXT_BYTES / 4);
    assert!(beyond_inline.len() > TEXT_BYTES);
    fs::write(f.path(), &beyond_inline).unwrap();
    assert!(matches!(read().unwrap(), ReadResult::Text { text, .. } if text == beyond_inline));
    f.clean();

    fs::write(f.path(), vec![b'x'; TRANSFER_TEXT_BYTES as usize + 1]).unwrap();
    assert_eq!(
        read().unwrap_err().message,
        "Text preview is limited to 8 MiB. Use Download for larger files."
    );
    f.clean();
}

#[test]
fn image_and_asset_reads_use_the_chunked_limit_and_release_rejected_transfers() {
    let f = Fixture::new();
    let cancel = EditorCancellation::default();
    let image = f.path().with_file_name("preview.png");
    fs::write(&image, b"\x89PNG\r\n\x1a\n").unwrap();
    let file = fs::OpenOptions::new().write(true).open(&image).unwrap();
    let location = Location {
        path: "preview.png".into(),
    };
    let asset = crate::AssetRequest {
        path: "document.md".into(),
        asset_path: location.path.clone(),
    };
    fs::write(f.path().with_file_name(&asset.path), "![preview](preview.png)").unwrap();
    let beyond_inline = 2 * 1024 * 1024 + 1;
    file.set_len(beyond_inline).unwrap();
    assert!(matches!(
        f.session.read_document(&location, &cancel).unwrap(),
        ReadResult::Image { size, mime_type, .. } if size == beyond_inline && mime_type == "image/png"
    ));
    assert_eq!(
        f.session.read_asset(&asset, cancel.clone()).unwrap().size,
        beyond_inline
    );
    file.set_len(TRANSFER_IMAGE_BYTES + 1).unwrap();
    for error in [
        f.session.read_document(&location, &cancel).unwrap_err(),
        f.session.read_asset(&asset, cancel.clone()).unwrap_err(),
    ] {
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(
            error.message,
            "Image and asset preview is limited to 64 MiB. Use Download for larger files."
        );
    }
    file.set_len(8).unwrap();
    assert_eq!(f.session.read_asset(&asset, cancel).unwrap().size, 8);
    drop(file);
    fs::remove_file(image).unwrap();
    f.clean();
}

#[test]
fn cancelled_chunk_read_and_save_release_every_transfer_and_keep_original_content() {
    let f = Fixture::new();
    let bytes = vec![b'x'; CHUNK_BYTES * 2 + 17];
    fs::write(f.path(), &bytes).unwrap();
    let cancel = EditorCancellation::default();
    assert_eq!(
        f.session
            .read_with(&f.location(), &cancel, |_| cancel.cancel())
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    f.clean();
    let file = f
        .session
        .read(&f.location(), &EditorCancellation::default())
        .unwrap();
    let cancel = EditorCancellation::default();
    assert_eq!(
        f.session
            .write_with(
                &WriteText {
                    path: file.path,
                    revision: file.revision,
                    text: "y".repeat(bytes.len())
                },
                &cancel,
                |_| cancel.cancel()
            )
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert_eq!(fs::read(f.path()).unwrap(), bytes);
    f.clean();
}
#[test]
fn conflict_reload_and_invalid_text_do_not_overwrite_external_saves() {
    let f = Fixture::new();
    fs::write(f.path(), "old\r\n雪").unwrap();
    let file = f
        .session
        .read(&f.location(), &EditorCancellation::default())
        .unwrap();
    fs::write(f.path(), "external\r\n雨").unwrap();
    assert_eq!(
        f.session
            .write(
                &WriteText {
                    path: file.path,
                    revision: file.revision,
                    text: "draft".into()
                },
                &EditorCancellation::default()
            )
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    let reloaded = f
        .session
        .read(&f.location(), &EditorCancellation::default())
        .unwrap();
    assert_eq!(reloaded.text, "external\r\n雨");
    assert_eq!(
        f.session
            .write(
                &WriteText {
                    path: reloaded.path,
                    revision: reloaded.revision,
                    text: "a\0b".into()
                },
                &EditorCancellation::default()
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    for invalid in [vec![255], vec![0]] {
        fs::write(f.path(), invalid).unwrap();
        assert_eq!(
            f.session
                .read(&f.location(), &EditorCancellation::default())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }
    f.clean();
}
