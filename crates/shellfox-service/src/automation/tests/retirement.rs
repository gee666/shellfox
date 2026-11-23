use super::*;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::Identity;
fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
// Private completed-state fixture. Native raw tests separately prove kernel
// cleanup; only the production close path can put real runs in this state.
fn completed(
    manager: &mut AutomationRuns,
    id: Uuid,
    identity: Identity,
    published: bool,
) -> AutomationView {
    let (source, publisher) = AutomationView::new();
    let input = AutomationStart {
        request_id: id,
        executable: "owned-fixture.exe".into(),
        args: vec![],
        cwd: std::env::current_dir().unwrap().to_str().unwrap().into(),
        env: BTreeMap::new(),
        title: None,
        detached: false,
    };
    let dto = AutomationRun {
        id,
        pid: 1,
        executable: input.executable.clone(),
        cwd: input.cwd.clone(),
        title: "fixture".into(),
        state: AutomationState::Exited,
        exit_code: Some(0),
        signal: None,
        session_id: published.then(|| identity.session_id.to_string().parse().unwrap()),
        tab_id: published.then(|| identity.tab_id.to_string().parse().unwrap()),
    };
    manager.runs.insert(
        id,
        Run {
            input,
            owner: 1,
            process: None,
            dto,
            frames: VecDeque::new(),
            bytes: 0,
            sequence: 12,
            acknowledged: 12,
            capturing: false,
            last_read: Instant::now(),
            input_ended: true,
            stopping: false,
            eof: [true; 2],
            outcome: Some(0),
            forced_stop: false,
            source: None,
            source_identity: Some(identity),
            publisher,
            pending: None,
            source_failure: None,
            failed_cleanup: None,
            launch_evidence: None,
        },
    );
    source
}
#[test]
fn typed_stdin_closed_never_masks_owner_or_missing_process_uncertainty() {
    let mut manager = AutomationRuns::new(std::env::current_exe().unwrap());
    let id = Uuid::new_v4();
    let _source = completed(&mut manager, id, identity(), false);
    let request = || AutomationWrite {
        id,
        data: String::new(),
        eof: false,
    };
    let closed = manager.begin_write(request(), 1).err().unwrap();
    assert!(closed.downcast_ref::<AutomationStdinClosed>().is_some());
    let denied = manager.begin_write(request(), 2).err().unwrap();
    assert!(denied.downcast_ref::<AutomationStdinClosed>().is_none());
    let run = manager.runs.get_mut(&id).unwrap();
    run.dto.state = AutomationState::Running;
    run.dto.exit_code = None;
    run.input_ended = false;
    let uncertain = manager.begin_write(request(), 1).err().unwrap();
    assert!(uncertain.downcast_ref::<AutomationStdinClosed>().is_none());
}
#[test]
fn retirement_is_bounded_and_failure_leaves_original_record_intact() {
    let mut manager = AutomationRuns::new(std::env::current_exe().unwrap());
    let mut sources = vec![];
    for _ in 0..AUTOMATION_RUNS {
        let id = Uuid::new_v4();
        sources.push(completed(&mut manager, id, identity(), false));
        manager.retire_for_admission(id).unwrap();
    }
    let id = Uuid::new_v4();
    sources.push(completed(&mut manager, id, identity(), false));
    assert!(manager.retire_for_admission(id).is_err());
    assert!(manager.runs.contains_key(&id));
    assert_eq!(manager.retired().len(), AUTOMATION_RUNS);
}
#[test]
fn exact_generation_disposition_and_receipt_nonce_guard_retirement_ack_and_reused_ids() {
    let mut manager = AutomationRuns::new(std::env::current_exe().unwrap());
    let id = Uuid::new_v4();
    let first = identity();
    let _source = completed(&mut manager, id, first, false);
    let input = manager.runs[&id].input.clone();
    manager.retire_for_admission(id).unwrap();
    let proof = manager.retired()[0].proof();
    assert!(proof.raw_cleanup_confirmed());
    assert_eq!(proof.run_id(), id);
    assert_eq!(proof.final_source_sequence().0, 12);
    assert_eq!(proof.runtime_identity(), first);
    assert!(manager.reserve_start(&input, 1).is_err());
    assert!(manager.is_stopped(id).unwrap());
    assert!(manager
        .ack_retired(
            proof,
            AutomationRetirementAck::UnshownParserClosed {
                identity: identity()
            }
        )
        .is_err());
    manager
        .ack_retired(
            proof,
            AutomationRetirementAck::UnshownParserClosed { identity: first },
        )
        .unwrap();
    let second = identity();
    let _source = completed(&mut manager, id, second, false);
    manager.retire_for_admission(id).unwrap();
    assert!(manager
        .ack_retired(
            proof,
            AutomationRetirementAck::UnshownParserClosed { identity: second }
        )
        .is_err());
    assert_eq!(manager.retired().len(), 1);
    assert_ne!(manager.retired()[0].proof(), proof);
}
#[test]
fn published_parser_transfers_immutable_proof_instead_of_being_destroyed() {
    let mut manager = AutomationRuns::new(std::env::current_exe().unwrap());
    let id = Uuid::new_v4();
    let canonical = identity();
    let source = completed(&mut manager, id, canonical, true);
    manager.retire_for_admission(id).unwrap();
    let receipt = manager.retired()[0];
    let proof = receipt.proof();
    assert!(receipt.publication().is_some());
    assert!(manager
        .ack_retired(
            proof,
            AutomationRetirementAck::UnshownParserClosed {
                identity: canonical
            }
        )
        .is_err());
    manager
        .ack_retired(
            proof,
            AutomationRetirementAck::PublishedParserTransferred {
                identity: canonical,
            },
        )
        .unwrap();
    assert!(manager.is_stopped(id).is_err());
    assert!(proof.raw_cleanup_confirmed());
    assert_eq!(proof.runtime_identity(), canonical);
    assert!(source.try_recv().is_none());
}
#[test]
fn missing_cleanup_or_exit_publication_never_mints_normal_retirement_proof() {
    let mut manager = AutomationRuns::new(std::env::current_exe().unwrap());
    let id = Uuid::new_v4();
    let _source = completed(&mut manager, id, identity(), false);
    manager.runs.get_mut(&id).unwrap().eof = [true, false];
    assert!(manager.retire_for_admission(id).is_err());
    let run = manager.runs.get_mut(&id).unwrap();
    run.eof = [true; 2];
    run.pending = Some(AutomationViewEvent::Exit {
        after_sequence: 12,
        code: 0,
    });
    assert!(manager.retire_for_admission(id).is_err());
    assert!(manager.retired().is_empty());
}
