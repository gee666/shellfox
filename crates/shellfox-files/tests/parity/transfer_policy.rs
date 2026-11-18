use super::streams::{actor, allow, capability, create};
use crate::support::Fixture;
use shellfox_files::*;
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use uuid::Uuid;

#[test]
fn transfer_ids_are_bound_to_actor_generation_capability_and_live_approval() {
    let f = Fixture::new();
    let cap = capability(&f);
    let owner = actor();
    let stale = TransferActor::new(owner.owner().into(), Uuid::new_v4()).unwrap();
    let mut registry = super::streams::ready(&f, TransferLimits::default());
    let approved = AtomicBool::new(true);
    let check = |_: &TransferActor, _: &FileService, _: bool| {
        if approved.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(FileError {
                code: ErrorCode::AccessDenied,
                message: "revoked".into(),
            })
        }
    };
    let started = registry
        .begin_write(&owner, cap.clone(), &create("never", 1), &check)
        .unwrap();
    let input = WriteChunk {
        transfer_id: started.transfer_id,
        offset: 0,
        data: vec![b'a'],
    };
    assert_eq!(
        registry
            .write_chunk(&stale, &input, &check)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        registry
            .write_chunk(&actor(), &input, &check)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    registry.write_chunk(&owner, &input, &check).unwrap();
    approved.store(false, Ordering::Release);
    assert_eq!(
        registry
            .finish_write(
                &owner,
                &TransferIdentity {
                    transfer_id: started.transfer_id
                },
                &check
            )
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
    approved.store(true, Ordering::Release);
    let read = registry.begin_read(
        &owner,
        cap.clone(),
        &BeginRead {
            path: ".".into(),
            asset_path: None,
        },
        &check,
    );
    assert!(read.is_err());
    let started = registry
        .begin_write(&owner, cap.clone(), &create("cancel", 0), &check)
        .unwrap();
    approved.store(false, Ordering::Release);
    registry
        .cancel(
            &owner,
            &TransferIdentity {
                transfer_id: started.transfer_id,
            },
        )
        .unwrap();
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
}

#[test]
fn source_changes_and_external_overwrites_conflict_without_retry() {
    let f = Fixture::new();
    let cap = capability(&f);
    let actor = actor();
    let mut registry = super::streams::ready(&f, TransferLimits::default());
    f.write("source", "first");
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
    f.write("source", "changed");
    assert_eq!(
        registry
            .read_chunk(
                &actor,
                &ReadChunk {
                    transfer_id: read.transfer_id,
                    offset: 0
                },
                &allow
            )
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    let mut input = create("source", 3);
    input.mode = WriteMode::Replace;
    input.revision = Some(
        f.service
            .read_text(&crate::support::loc("source"))
            .unwrap()
            .revision,
    );
    let started = registry.begin_write(&actor, cap, &input, &allow).unwrap();
    registry
        .write_chunk(
            &actor,
            &WriteChunk {
                transfer_id: started.transfer_id,
                offset: 0,
                data: b"new".to_vec(),
            },
            &allow,
        )
        .unwrap();
    f.write("source", "external");
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
        ErrorCode::FileConflict
    );
    assert_eq!(f.read("source"), b"external");
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
}

#[test]
fn resources_have_per_owner_global_disk_receipt_and_idle_bounds() {
    let f = Fixture::new();
    let cap = capability(&f);
    let mut registry = super::streams::ready(
        &f,
        TransferLimits {
            max_file_bytes: 10,
            idle_ttl: Duration::from_secs(300),
        },
    );
    let mut owners = Vec::new();
    for owner in 0..4 {
        let actor = actor();
        for slot in 0..2 {
            registry
                .begin_write(
                    &actor,
                    cap.clone(),
                    &create(&format!("{owner}-{slot}"), 0),
                    &allow,
                )
                .unwrap();
        }
        assert_eq!(
            registry
                .begin_write(&actor, cap.clone(), &create("over-owner", 0), &allow)
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
        owners.push(actor);
    }
    assert_eq!(registry.usage().active, TRANSFERS_TOTAL);
    assert_eq!(
        registry
            .begin_write(&actor(), cap.clone(), &create("over-global", 0), &allow)
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    for owner in owners {
        registry.cancel_actor(owner.owner());
    }
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
    let owner = actor();
    let other = actor();
    registry
        .begin_write(&owner, cap.clone(), &create("reserve", 10), &allow)
        .unwrap();
    registry
        .begin_write(&other, cap.clone(), &create("reserve2", 10), &allow)
        .unwrap();
    assert_eq!(
        registry
            .begin_write(&actor(), cap.clone(), &create("reserve3", 1), &allow)
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    registry.dispose();
    let mut registry = super::streams::ready(
        &f,
        TransferLimits {
            max_file_bytes: 1,
            idle_ttl: Duration::from_millis(1),
        },
    );
    registry
        .begin_write(&owner, cap.clone(), &create("expire", 1), &allow)
        .unwrap();
    std::thread::sleep(Duration::from_millis(10));
    registry.sweep(&allow).unwrap();
    assert_eq!(registry.usage().active, 0);
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
    registry.dispose();
    let mut registry = super::streams::ready(&f, TransferLimits::default());
    for index in 0..TRANSFER_RECEIPTS + 2 {
        let write = registry
            .begin_write(
                &owner,
                cap.clone(),
                &create(&format!("done-{index}"), 0),
                &allow,
            )
            .unwrap();
        registry
            .finish_write(
                &owner,
                &TransferIdentity {
                    transfer_id: write.transfer_id,
                },
                &allow,
            )
            .unwrap();
    }
    assert_eq!(registry.usage().receipts, TRANSFER_RECEIPTS);
}
