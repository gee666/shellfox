use super::*;
use shellfox_core::{Generation, SessionId, TabId};

fn handle() -> (Runtime, Receiver<Request>, Arc<Shared>) {
    let (commands, incoming) = mpsc::sync_channel(COMMAND_CAPACITY);
    let (_, events) = mpsc::sync_channel(EVENT_CAPACITY);
    let shared = Arc::new(Shared::new(LiveOutputStatus::Disabled, Phase::Active));
    let runtime = Runtime {
        identity: Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        },
        limits: GridLimits::default(),
        launch: launch::new(LaunchFactoryPurpose::OwnedPtyProgram).0,
        commands,
        events,
        shared: Arc::clone(&shared),
        inspection: None,
        source_kind: SourceKind::OwnedPty,
        external_source: None,
        external_status: None,
        external_control: None,
    };
    (runtime, incoming, shared)
}
#[test]
fn full_queue_returns_the_exact_unsent_request_and_drop_still_requests_cleanup() {
    let (runtime, _incoming, shared) = handle();
    for id in 0..COMMAND_CAPACITY {
        runtime
            .try_send(Request {
                id: RequestId(id as u64),
                command: Command::RetryClose,
            })
            .unwrap();
    }
    let error = runtime
        .try_send(Request {
            id: RequestId(99),
            command: Command::RetryClose,
        })
        .unwrap_err();
    assert!(matches!(
        error,
        SubmitError::Full(Request {
            id: RequestId(99),
            command: Command::RetryClose
        })
    ));
    drop(runtime);
    assert!(shared.abandoned.load(Ordering::Acquire));
    // The handle does not misreport cleanup on the owner's behalf.
    assert_eq!(*shared.phase.lock().unwrap(), Phase::Active);
}
#[test]
fn invalid_input_does_not_consume_bounded_command_credit() {
    let (runtime, incoming, _) = handle();
    for (cols, rows) in [(0, 1), (1, 0), (32768, 1), (1, 32768), (1024, 1024)] {
        let error = runtime
            .try_send(Request {
                id: RequestId(1),
                command: Command::Resize {
                    lease: Lease {
                        controller: 1,
                        epoch: 1,
                    },
                    cols,
                    rows,
                },
            })
            .unwrap_err();
        assert!(matches!(error, SubmitError::Invalid(_)));
    }
    assert!(matches!(incoming.try_recv(), Err(TryRecvError::Empty)));
}
#[test]
fn grid_limits_support_high_dpi_and_reject_invalid_resizes_before_queueing() {
    let limits = GridLimits::default();
    assert_eq!(limits.max_cells(), 262144);
    for (cols, rows) in [(480, 120), (500, 250), (960, 240)] {
        limits.validate_size(cols, rows).unwrap();
    }
    assert!(limits.validate_size(1024, 1024).is_err());
    GridLimits::new(MAX_SCREEN_CELLS)
        .unwrap()
        .validate_size(1024, 1024)
        .unwrap();
    assert!(GridLimits::new(0).is_err());
    assert!(GridLimits::new(MAX_SCREEN_CELLS + 1).is_err());
    let (mut runtime, incoming, _) = handle();
    runtime.limits = GridLimits::new(80 * 24).unwrap();
    let error = runtime
        .try_send(Request {
            id: RequestId(9),
            command: Command::Resize {
                lease: Lease {
                    controller: 1,
                    epoch: 1,
                },
                cols: 100,
                rows: 30,
            },
        })
        .unwrap_err();
    assert!(matches!(
        error,
        SubmitError::Invalid(RuntimeError {
            code: ErrorCode::Validation,
            ..
        })
    ));
    assert!(matches!(incoming.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(runtime.phase(), Phase::Active);
}
#[test]
fn disconnected_command_queue_returns_the_unsent_intent() {
    let (runtime, incoming, _) = handle();
    drop(incoming);
    assert!(matches!(
        runtime.try_send(Request {
            id: RequestId(5),
            command: Command::RetryClose
        }),
        Err(SubmitError::Disconnected(Request {
            id: RequestId(5),
            command: Command::RetryClose
        }))
    ));
}
