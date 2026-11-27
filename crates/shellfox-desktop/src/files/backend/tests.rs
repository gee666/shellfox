use super::*;
use shellfox_files::Access;
use std::{fs, path::Path};

#[test]
fn typed_file_reveals_parent_and_missing_path_is_not_created_by_lookup() {
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .canonicalize()
        .unwrap();
    let owned = tempfile::tempdir_in(tmp.to_str().unwrap().trim_start_matches("\\\\?\\")).unwrap();
    let root = owned.path().join("project");
    fs::create_dir(&root).unwrap();
    let file = root.join("reports.txt");
    fs::write(&file, "Report\n").unwrap();
    let service = Arc::new(FileService::approved_root(&root, Access::ReadWrite).unwrap());
    let backend = EditorBackend::new(service).unwrap();
    let local = LocalDocumentBackend {
        service: backend.capability(),
        session: backend.session(),
    };
    assert!(matches!(
        local.lookup_path(root.to_str().unwrap()).unwrap(),
        Target::Directory(_)
    ));
    assert!(matches!(local.lookup_path(file.to_str().unwrap()).unwrap(),
        Target::File { parent, path } if Path::new(&parent.path) == root && Path::new(&path) == file));
    let missing = root.join("new/child");
    assert!(
        matches!(local.lookup_path(missing.to_str().unwrap()).unwrap(), Target::Missing(path) if Path::new(&path) == missing)
    );
    assert!(!root.join("new").exists());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let list = runtime
        .block_on(
            DocumentBackend::Local(Arc::new(local)).create_directory(missing.to_str().unwrap()),
        )
        .unwrap();
    assert_eq!(Path::new(&list.path), missing);
    assert!(missing.is_dir());
}

#[test]
fn new_file_is_empty_and_never_replaces_an_existing_entry() {
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .canonicalize()
        .unwrap();
    let owned = tempfile::tempdir_in(tmp.to_str().unwrap().trim_start_matches("\\\\?\\")).unwrap();
    let root = owned.path().join("project");
    fs::create_dir(&root).unwrap();
    let service = Arc::new(FileService::approved_root(&root, Access::ReadWrite).unwrap());
    let backend = EditorBackend::new(service).unwrap();
    let backend = DocumentBackend::local(&Arc::new(backend));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let file = root.join("notes.md");
    let created = runtime
        .block_on(backend.create_file(file.to_str().unwrap()))
        .unwrap();
    assert_eq!(Path::new(&created), file);
    assert_eq!(fs::read(&file).unwrap(), b"");
    fs::write(&file, "kept").unwrap();
    let error = runtime
        .block_on(backend.create_file(file.to_str().unwrap()))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "A file already exists at this destination."
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "kept");
}
