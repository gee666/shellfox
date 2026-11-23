use super::*;
use shellfox_protocol::{ErrorCode, Validate};

fn fixture(permission: ViewPermission) -> (tempfile::TempDir, Actor, RemoteTransfers) {
    let (scratch, actor) = crate::test_support::actor(permission);
    let transfers = RemoteTransfers::open(Default::default()).unwrap();
    (scratch, actor, transfers)
}
fn run(
    transfers: &mut RemoteTransfers,
    actor: &Actor,
    command: RemoteCommand,
) -> Result<serde_json::Value, AppError> {
    actor.with_authorized(Some(&command), || transfers.request(actor, &command))
}
#[test]
fn v1_chunked_read_write_and_receipts_use_real_approved_file_capabilities() {
    let (scratch, actor, mut transfers) = fixture(ViewPermission::Write);
    let file = scratch.path().join("file.txt");
    let path = file.to_str().unwrap().to_owned();
    let started = run(
        &mut transfers,
        &actor,
        RemoteCommand::BeginFileWrite(wire::BeginFileWrite {
            session_id: None,
            tab_id: None,
            path: path.clone(),
            size: 5,
            mode: wire::FileWriteMode::Create,
            revision: None,
            content: wire::FileContent::Text,
            mime_type: None,
        }),
    )
    .unwrap();
    let id = serde_json::from_value::<wire::FileWriteTransfer>(started)
        .unwrap()
        .transfer_id;
    let result = run(
        &mut transfers,
        &actor,
        RemoteCommand::WriteFileChunk(wire::WriteFileChunk {
            transfer_id: id,
            offset: 0,
            data: wire::encode_chunk(b"hello").unwrap(),
        }),
    )
    .unwrap();
    assert_eq!(result["offset"], 5);
    let finished = run(
        &mut transfers,
        &actor,
        RemoteCommand::FinishFileWrite(wire::FinishFileWrite { transfer_id: id }),
    )
    .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"hello");
    assert_eq!(
        finished,
        run(
            &mut transfers,
            &actor,
            RemoteCommand::FinishFileWrite(wire::FinishFileWrite { transfer_id: id })
        )
        .unwrap()
    );
    let read = run(
        &mut transfers,
        &actor,
        RemoteCommand::BeginFileRead(wire::BeginFileRead {
            session_id: None,
            tab_id: None,
            path,
            asset_path: None,
        }),
    )
    .unwrap();
    let read = serde_json::from_value::<wire::FileReadTransfer>(read).unwrap();
    read.validate().unwrap();
    let chunk = run(
        &mut transfers,
        &actor,
        RemoteCommand::ReadFileChunk(wire::ReadFileChunk {
            transfer_id: read.transfer_id,
            offset: 0,
        }),
    )
    .unwrap();
    assert_eq!(
        wire::decode_chunk(chunk["data"].as_str().unwrap()).unwrap(),
        b"hello"
    );
    assert_eq!(chunk["eof"], true);
    let mut replacement = actor.clone();
    replacement.view_epoch = 2;
    assert_eq!(
        run(
            &mut transfers,
            &replacement,
            RemoteCommand::ReadFileChunk(wire::ReadFileChunk {
                transfer_id: read.transfer_id,
                offset: 0
            })
        )
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    assert!(run(
        &mut transfers,
        &actor,
        RemoteCommand::CancelFileTransfer(wire::CancelFileTransfer {
            transfer_id: read.transfer_id
        })
    )
    .is_ok());
}
#[test]
fn migrated_approved_device_uses_full_account_while_cas_and_ambient_path_safety_remain() {
    let (scratch, actor, mut transfers) = fixture(ViewPermission::ReadOnly);
    let file = scratch.path().join("file.txt");
    std::fs::write(&file, b"original").unwrap();
    let command = RemoteCommand::BeginFileWrite(wire::BeginFileWrite {
        session_id: None,
        tab_id: None,
        path: file.to_str().unwrap().into(),
        size: 1,
        mode: wire::FileWriteMode::Create,
        revision: None,
        content: wire::FileContent::Text,
        mime_type: None,
    });
    assert_eq!(
        run(&mut transfers, &actor, command).unwrap_err().code,
        ErrorCode::FileConflict
    );
    assert_eq!(actor.permission, ViewPermission::Write);
    assert_eq!(
        actor.file_capabilities()[0].capability_kind(),
        files::CapabilityKind::ApprovedFullAccount
    );
    let command = RemoteCommand::BeginFileRead(wire::BeginFileRead {
        session_id: None,
        tab_id: None,
        path: "../outside.txt".into(),
        asset_path: None,
    });
    assert!(run(&mut transfers, &actor, command).is_err());
    assert_eq!(std::fs::read(file).unwrap(), b"original");
}
