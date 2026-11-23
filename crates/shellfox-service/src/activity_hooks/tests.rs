use crate::*;
use shellfox_core::{CreateSession, ProfileId, SessionId};
#[cfg(windows)]
use shellfox_host::CapturedSettlement;

fn fixture() -> (tempfile::TempDir, Service, shellfox_core::Snapshot) {
    let data = crate::tests::scratch();
    let service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let snapshot = service
        .host()
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    (data, service, snapshot)
}
#[test]
fn pending_activity_survives_failed_commit_and_flushes_only_after_success() {
    let (_data, mut service, snapshot) = fixture();
    let id = snapshot.sessions[0].id;
    let at = shellfox_host::activity_unix_ms(&snapshot.sessions[0].updated_at).unwrap() + 1000;
    service.activity.persistence.touch(id, at).unwrap();
    let blocked = service.host.prepare_delete_session(id).unwrap();
    assert!(service.flush_activity(true).is_err());
    assert_eq!(service.activity.persistence.pending_count(), 1);
    service.host.cancel_close(blocked.token).unwrap();
    service.flush_activity(true).unwrap();
    assert_eq!(service.activity.persistence.pending_count(), 0);
    assert_eq!(
        shellfox_host::activity_unix_ms(&service.host.snapshot().unwrap().sessions[0].updated_at)
            .unwrap(),
        at
    );
    assert!(matches!(
        service.shutdown().unwrap().activity,
        ActivityShutdownState::Committed
    ));
}
#[test]
fn ownership_loss_confirms_resources_but_reports_uncommitted_activity_separately() {
    let (_data, mut service, snapshot) = fixture();
    let id = snapshot.sessions[0].id;
    service
        .activity
        .persistence
        .touch(id, service.activity.now_ms().unwrap())
        .unwrap();
    service.ownership_lost.cancel();
    let outcome = service.shutdown().unwrap();
    assert_eq!(
        outcome.activity,
        ActivityShutdownState::DirtyAfterOwnershipLoss {
            pending_sessions: 1
        }
    );
    assert_eq!(service.activity.persistence.pending_count(), 1);
    assert_eq!(
        service.host.snapshot().unwrap().sessions[0].updated_at,
        snapshot.sessions[0].updated_at
    );
}

#[cfg(windows)]
#[test]
#[ignore = "requires a trusted separate supervisor artifact; no installed data or host"]
fn positive_receipts_keep_original_settlement_and_metadata_failure_never_fails_write() {
    use crate::activity_integration::InputAdmission;
    use shellfox_runtime::{Command, InputKind, Response};
    let data = crate::tests::scratch();
    let supervisor = std::path::PathBuf::from(
        std::env::var_os("SHELLFOX_NATIVE_TEST_SUPERVISOR").expect("supervisor artifact"),
    );
    let mut service = Service::open_owned(data.path(), &supervisor).unwrap();
    let snapshot = service
        .host()
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let session = snapshot.sessions[0].id;
    let tab = snapshot.sessions[0].tabs[0].id;
    let captured_null = CapturedSettlement::Captured(snapshot.sessions[0].settled_at.clone());
    let result = (|| -> Result<()> {
        let attachment = service.attach(tab, 1, false)?;
        let mut settled = service.host.set_session_settled(session, true)?.sessions[0]
            .settled_at
            .clone();
        let lease = service.lease(tab, &attachment)?;
        let write = |service: &mut Service,
                     bytes: &[u8],
                     captured: CapturedSettlement|
         -> Result<shellfox_runtime::InputReceipt> {
            let admission = InputAdmission::raw(bytes, true, true, captured)
                .map_err(|_| anyhow::anyhow!("admission"))?;
            let Response::Input(receipt) = service.call_input(
                tab,
                Command::RawInput {
                    lease,
                    bytes: bytes.to_vec(),
                },
                admission,
            )?
            else {
                bail!("input receipt required");
            };
            assert_eq!(receipt.kind, InputKind::Raw);
            assert_eq!(receipt.accepted_bytes as usize, bytes.len());
            Ok(receipt)
        };
        assert_eq!(
            write(&mut service, b"", CapturedSettlement::Legacy)?.accepted_bytes,
            0
        );
        assert_eq!(service.host.snapshot()?.sessions[0].settled_at, settled);
        write(&mut service, b"echo captured-null\r", captured_null)?;
        assert_eq!(service.host.snapshot()?.sessions[0].settled_at, settled);
        let stale = CapturedSettlement::Captured(settled.clone());
        service.host.set_session_settled(session, false)?;
        let newer = service.host.set_session_settled(session, true)?.sessions[0]
            .settled_at
            .clone();
        assert_ne!(newer, settled);
        settled = newer;
        write(&mut service, b"echo captured-stale\r", stale)?;
        assert_eq!(service.host.snapshot()?.sessions[0].settled_at, settled);
        let blocked = service.host.prepare_close_tab(tab)?;
        write(
            &mut service,
            b"echo metadata-blocked\r",
            CapturedSettlement::Captured(settled.clone()),
        )?;
        assert_eq!(service.host.snapshot()?.sessions[0].settled_at, settled);
        assert!(service.activity.persistence.pending_count() > 0);
        service.host.cancel_close(blocked.token)?;
        write(
            &mut service,
            b"exit\r",
            CapturedSettlement::Captured(settled.clone()),
        )?;
        assert_eq!(service.host.snapshot()?.sessions[0].settled_at, None);
        Ok(())
    })();
    let cleanup = service.shutdown();
    cleanup.expect("owned process cleanup and activity flush");
    result.unwrap();
}
