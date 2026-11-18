use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
};
struct Fixture {
    root: PathBuf,
    session: EditorSession,
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
            session: EditorSession::new(capability, Arc::new(Mutex::new(registry))),
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
        drop(std::mem::replace(&mut self.session, empty));
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
