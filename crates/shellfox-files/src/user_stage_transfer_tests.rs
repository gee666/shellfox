use super::*;
use crate::{
    BeginRead, BeginWrite, ReadChunk, TransferActor, TransferContent, TransferIdentity, WriteChunk,
    WriteMode, CHUNK_BYTES, TRANSFER_TEXT_BYTES,
};
fn allowed(_: &TransferActor, cap: &FileService, write: bool) -> Result<()> {
    cap.authorized(write)
}
#[test]
fn v2_exact_eight_mib_generic_transfer_create_download_and_cached_finish() {
    let mut f = Fixture::new();
    let actor = TransferActor::new("owned-stream".into(), Uuid::new_v4()).unwrap();
    let cap = f.cap.clone();
    let bytes = vec![0xa5; TRANSFER_TEXT_BYTES as usize];
    let registry = f.registry.as_mut().unwrap();
    let started = registry
        .begin_write(
            &actor,
            cap.clone(),
            &BeginWrite {
                path: "binary.bin".into(),
                size: bytes.len() as u64,
                mode: WriteMode::Create,
                revision: None,
                content: TransferContent::File,
                mime_type: None,
            },
            &allowed,
        )
        .unwrap();
    for (offset, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
        registry
            .write_chunk(
                &actor,
                &WriteChunk {
                    transfer_id: started.transfer_id,
                    offset: (offset * CHUNK_BYTES) as u64,
                    data: chunk.into(),
                },
                &allowed,
            )
            .unwrap();
    }
    let identity = TransferIdentity {
        transfer_id: started.transfer_id,
    };
    let finished = registry.finish_write(&actor, &identity, &allowed).unwrap();
    assert_eq!(
        registry
            .finish_write(&actor, &identity, &allowed)
            .unwrap()
            .revision,
        finished.revision
    );
    let read = registry
        .begin_read(
            &actor,
            cap,
            &BeginRead {
                path: "binary.bin".into(),asset_path:None,
            },
            &allowed,
        )
        .unwrap();
    let mut offset = 0;
    let mut downloaded = Vec::new();
    loop {
        let chunk = registry
            .read_chunk(
                &actor,
                &ReadChunk {
                    transfer_id: read.transfer_id,
                    offset,
                },
                &allowed,
            )
            .unwrap();
        offset = chunk.offset;
        downloaded.extend(chunk.data);
        if chunk.eof {
            break;
        }
    }
    registry
        .cancel(
            &actor,
            &TransferIdentity {
                transfer_id: read.transfer_id,
            },
        )
        .unwrap();
    assert_eq!(downloaded, bytes);
    assert_eq!(fs::read(f.root.join("user/binary.bin")).unwrap(), bytes);
    assert_eq!(f.journal().lock().unwrap().entries.len(), 0);
}
#[test]
fn v2_transfer_cancel_and_revocation_never_publish_or_clean_an_unrelated_file() {
    let mut f = Fixture::new();
    fs::write(f.root.join("user/unrelated"), b"keep").unwrap();
    let actor = TransferActor::new("owned-cancel".into(), Uuid::new_v4()).unwrap();
    let cap = f.cap.clone();
    let registry = f.registry.as_mut().unwrap();
    let started = registry
        .begin_write(
            &actor,
            cap.clone(),
            &BeginWrite {
                path: "cancelled.bin".into(),
                size: CHUNK_BYTES as u64,
                mode: WriteMode::Create,
                revision: None,
                content: TransferContent::File,
                mime_type: None,
            },
            &allowed,
        )
        .unwrap();
    registry
        .write_chunk(
            &actor,
            &WriteChunk {
                transfer_id: started.transfer_id,
                offset: 0,
                data: vec![42; CHUNK_BYTES],
            },
            &allowed,
        )
        .unwrap();
    cap.revoke().unwrap();
    registry
        .cancel(
            &actor,
            &TransferIdentity {
                transfer_id: started.transfer_id,
            },
        )
        .unwrap();
    assert!(!f.root.join("user/cancelled.bin").exists());
    assert_eq!(fs::read(f.root.join("user/unrelated")).unwrap(), b"keep");
    assert_eq!(fs::read_dir(f.root.join("user")).unwrap().count(), 1);
    assert!(f.journal().lock().unwrap().entries.is_empty());
}
