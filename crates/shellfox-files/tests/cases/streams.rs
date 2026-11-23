use crate::support::{loc, Fixture};
use sha2::{Digest, Sha256};
use shellfox_files::*;
use std::{fs, sync::Arc};
use uuid::Uuid;

pub(super) fn ready(limits: TransferLimits) -> TransferRegistry {
    TransferRegistry::new(limits).unwrap()
}
pub(super) fn actor() -> TransferActor {
    TransferActor::new(Uuid::new_v4().to_string(), Uuid::new_v4()).unwrap()
}
pub(super) fn allow(_: &TransferActor, _: &FileService, _: bool) -> Result<()> {
    Ok(())
}
pub(super) fn capability(f: &Fixture) -> Arc<FileService> {
    Arc::new(FileService::approved_root(&f.root, Access::ReadWrite).unwrap())
}
pub(super) fn create(path: &str, size: u64) -> BeginWrite {
    BeginWrite {
        path: path.into(),
        size,
        mode: WriteMode::Create,
        revision: None,
        content: TransferContent::File,
        mime_type: None,
    }
}

#[test]
fn chunk_streams_are_bounded_ordered_hashed_and_replay_only_exact_last_chunk() {
    let f = Fixture::new();
    let cap = capability(&f);
    let mut registry = ready(TransferLimits::default());
    let actor = actor();
    let bytes: Vec<u8> = (0..CHUNK_BYTES * 2 + 13)
        .map(|index| (index % 251) as u8)
        .collect();
    f.write("source", &bytes);
    let read = registry
        .begin_read(
            &actor,
            cap.clone(),
            &BeginRead {
                path: "source".into(),
                asset_path: None,
            },
            &allow,
        )
        .unwrap();
    assert_eq!(read.chunk_bytes, CHUNK_BYTES);
    assert_eq!(read.revision, format!("{:x}", Sha256::digest(&bytes)));
    let mut output = Vec::new();
    let mut offset = 0;
    loop {
        let input = ReadChunk {
            transfer_id: read.transfer_id,
            offset,
        };
        let result = registry.read_chunk(&actor, &input, &allow).unwrap();
        assert!(result.data.len() <= CHUNK_BYTES);
        assert_eq!(
            registry.read_chunk(&actor, &input, &allow).unwrap().data,
            result.data
        );
        output.extend_from_slice(&result.data);
        offset = result.offset;
        if result.eof {
            break;
        }
    }
    assert_eq!(output, bytes);
    registry
        .cancel(
            &actor,
            &TransferIdentity {
                transfer_id: read.transfer_id,
            },
        )
        .unwrap();
    let write = registry
        .begin_write(
            &actor,
            cap.clone(),
            &create("destination", bytes.len() as u64),
            &allow,
        )
        .unwrap();
    for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
        let input = WriteChunk {
            transfer_id: write.transfer_id,
            offset: (index * CHUNK_BYTES) as u64,
            data: chunk.to_vec(),
        };
        let progress = registry.write_chunk(&actor, &input, &allow).unwrap();
        assert_eq!(
            registry.write_chunk(&actor, &input, &allow).unwrap().offset,
            progress.offset
        );
    }
    let id = TransferIdentity {
        transfer_id: write.transfer_id,
    };
    let saved = registry.finish_write(&actor, &id, &allow).unwrap();
    assert_eq!(saved.revision, read.revision);
    assert_eq!(f.read("destination"), bytes);
    f.write("destination", "external-later");
    assert_eq!(
        registry.finish_write(&actor, &id, &allow).unwrap().revision,
        saved.revision
    );
    assert_eq!(f.read("destination"), b"external-later");
    assert_eq!(registry.usage().active, 0);
}

#[test]
fn utf8_codepoints_cross_chunk_boundaries_and_large_editor_saves_work() {
    let f = Fixture::new();
    let cap = capability(&f);
    let actor = actor();
    let mut registry = ready(TransferLimits::default());
    let text = format!("{}雪\u{feff}\r\n", "x".repeat(CHUNK_BYTES - 1));
    f.write("large.md", &text);
    assert_eq!(
        f.service.read_text(&loc("large.md")).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let read = registry
        .begin_read(
            &actor,
            cap.clone(),
            &BeginRead {
                path: "large.md".into(),
                asset_path: None,
            },
            &allow,
        )
        .unwrap();
    assert_eq!(read.kind, ContentKind::Text);
    assert_eq!(read.mime_type, "text/markdown");
    registry
        .cancel(
            &actor,
            &TransferIdentity {
                transfer_id: read.transfer_id,
            },
        )
        .unwrap();
    let mut input = create("new.md", text.len() as u64);
    input.content = TransferContent::Text;
    let write = registry.begin_write(&actor, cap, &input, &allow).unwrap();
    for (index, chunk) in text.as_bytes().chunks(CHUNK_BYTES).enumerate() {
        registry
            .write_chunk(
                &actor,
                &WriteChunk {
                    transfer_id: write.transfer_id,
                    offset: (index * CHUNK_BYTES) as u64,
                    data: chunk.to_vec(),
                },
                &allow,
            )
            .unwrap();
    }
    registry
        .finish_write(
            &actor,
            &TransferIdentity {
                transfer_id: write.transfer_id,
            },
            &allow,
        )
        .unwrap();
    assert_eq!(f.read("new.md"), text.as_bytes());
}

#[test]
fn bad_offsets_incomplete_or_non_utf8_writes_leave_no_destination_or_stage() {
    let f = Fixture::new();
    let cap = capability(&f);
    let actor = actor();
    let mut registry = ready(TransferLimits::default());
    let started = registry
        .begin_write(&actor, cap.clone(), &create("bad", 2), &allow)
        .unwrap();
    assert_eq!(
        registry
            .write_chunk(
                &actor,
                &WriteChunk {
                    transfer_id: started.transfer_id,
                    offset: 1,
                    data: vec![1]
                },
                &allow
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    let started = registry
        .begin_write(&actor, cap.clone(), &create("incomplete", 2), &allow)
        .unwrap();
    assert_eq!(
        registry
            .finish_write(
                &actor,
                &TransferIdentity {
                    transfer_id: started.transfer_id
                },
                &allow
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    let mut input = create("contains-nul", 1);
    input.content = TransferContent::Text;
    let started = registry.begin_write(&actor, cap, &input, &allow).unwrap();
    registry
        .write_chunk(
            &actor,
            &WriteChunk {
                transfer_id: started.transfer_id,
                offset: 0,
                data: vec![0],
            },
            &allow,
        )
        .unwrap();
    assert_eq!(
        registry
            .finish_write(
                &actor,
                &TransferIdentity {
                    transfer_id: started.transfer_id
                },
                &allow
            )
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
}
