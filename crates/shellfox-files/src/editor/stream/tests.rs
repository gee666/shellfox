use super::*;
use std::{fs, path::Path};

#[test]
fn picker_streams_preserve_bytes_and_leave_existing_file_on_cancellation() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .canonicalize()
        .unwrap();
    let base = base.to_str().unwrap().trim_start_matches("\\\\?\\");
    let root = Path::new(base).join(format!("rust-file-picker-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("files")).unwrap();
    let capability =
        Arc::new(FileService::approved_root(root.join("files"), Access::ReadWrite).unwrap());
    let backend = EditorBackend::new(capability).unwrap();
    let session = backend.session();
    let input = vec![255; CHUNK_BYTES + 19];
    fs::write(root.join("files/from.bin"), &input).unwrap();
    fs::write(root.join("files/to.bin"), b"existing").unwrap();

    let cancel = EditorCancellation::default();
    let mut source = session.file_source("from.bin", cancel.clone()).unwrap();
    assert_eq!(source.header().size, input.len() as u64);
    let mut bytes = Vec::new();
    source.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, input);
    drop(source);

    let mut sink = session
        .file_sink("to.bin", bytes.len() as u64, cancel.clone())
        .unwrap();
    sink.append(&bytes[..CHUNK_BYTES]).unwrap();
    cancel.cancel();
    assert!(sink.append(&bytes[CHUNK_BYTES..]).is_err());
    drop(sink);
    assert_eq!(fs::read(root.join("files/to.bin")).unwrap(), b"existing");

    let mut sink = session
        .file_sink("to.bin", bytes.len() as u64, EditorCancellation::default())
        .unwrap();
    for chunk in bytes.chunks(CHUNK_BYTES) {
        sink.append(chunk).unwrap();
    }
    sink.finish().unwrap();
    assert_eq!(fs::read(root.join("files/to.bin")).unwrap(), bytes);
    assert_eq!(session.registry.lock().unwrap().usage().active, 0);
    assert_eq!(fs::read_dir(root.join("files")).unwrap().count(), 2);
    drop(session);
    drop(backend);

    let full = EditorBackend::new(Arc::new(FileService::approved_full_account())).unwrap();
    let session = full.session();
    let created = root.join("files/new.bin");
    let mut sink = session
        .file_sink(
            created.to_str().unwrap(),
            bytes.len() as u64,
            EditorCancellation::default(),
        )
        .unwrap();
    for chunk in bytes.chunks(CHUNK_BYTES) {
        sink.append(chunk).unwrap();
    }
    sink.finish().unwrap();
    assert_eq!(fs::read(created).unwrap(), bytes);
    assert_eq!(session.registry.lock().unwrap().usage().active, 0);
    drop(session);
    drop(full);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn asset_reads_have_their_own_budget_beside_user_transfers() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .canonicalize()
        .unwrap();
    let base = base.to_str().unwrap().trim_start_matches("\\\\?\\");
    let root = Path::new(base).join(format!("rust-asset-budget-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("files")).unwrap();
    fs::write(root.join("files/page.html"), "<img src=a.png>").unwrap();
    fs::write(root.join("files/a.png"), b"png").unwrap();
    fs::write(root.join("files/big.bin"), b"data").unwrap();
    let capability =
        Arc::new(FileService::approved_root(root.join("files"), Access::ReadWrite).unwrap());
    let backend = EditorBackend::new(capability).unwrap();
    let session = backend.session();
    let cancel = EditorCancellation::default();
    // The user's own transfers fill their quota.
    let downloads: Vec<_> = (0..TRANSFERS_PER_ACTOR)
        .map(|_| session.file_source("big.bin", cancel.clone()).ok().unwrap())
        .collect();
    assert!(session.file_source("big.bin", cancel.clone()).is_err());
    let asset = |_| {
        session.open_source(
            BeginRead {
                path: "page.html".into(),
                asset_path: Some("a.png".into()),
            },
            cancel.clone(),
        )
    };
    let previews: Vec<_> = (0..ASSET_READS_PER_ACTOR)
        .map(|i| asset(i).ok().unwrap())
        .collect();
    assert_eq!(asset(0).err().unwrap().code, ErrorCode::Validation);
    drop(previews);
    assert_eq!(
        session
            .read_asset(
                &AssetRequest {
                    path: "page.html".into(),
                    asset_path: "a.png".into(),
                },
                cancel.clone()
            )
            .unwrap()
            .bytes,
        b"png"
    );
    drop(downloads);
    assert_eq!(session.registry.lock().unwrap().usage().active, 0);
    drop(session);
    drop(backend);
    fs::remove_dir_all(root).unwrap();
}
