//! Real owned Runtime/production delivery mapper/Rust SDK queue boundary.
//! In-memory authenticated-transport fixture, NOT Go/Tailscale/native approval proof.
//! Uses only an explicitly selected private supervisor and project-owned PTY.
#![cfg(all(
    any(windows, target_os = "linux"),
    feature = "experimental-shellfox-native"
))]
#[path = "../src/manager_delivery.rs"]
pub mod manager_delivery;
use manager_delivery::{ManagerDelivery, Role};
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_ghostty_vt::extension::{RestorationConfig, SnapshotLimits};
use shellfox_mobile::ManagerRecovery;
use shellfox_protocol::{manager_v2 as m, *};
use shellfox_runtime::{
    Command, Event, Identity, Lease, Phase, Request, RequestId, Response, Runtime, RuntimeHooks,
};
use shellfox_terminal::{PtyCommand, PtyConfig};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
fn id(n: u8) -> m::Id {
    format!("{n}0000000-0000-4000-8000-000000000001")
        .parse()
        .unwrap()
}
#[path = "support/native_manager_recovery_transport.rs"]
mod transport_fixture;
use transport_fixture::{Server, Transport};
#[test]
#[ignore = "requires explicit owned Runtime supervisor fixture + private Native002 artifact and Root execution permission; never the shared Service-lib executable"]
fn real_cut_mapping_install_consumption_opens_only_original_runtime_credit() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&parent).unwrap();
    let root = tempfile::Builder::new()
        .prefix("native-manager-boundary-")
        .tempdir_in(parent)
        .unwrap()
        .keep();
    let executable = PathBuf::from(
        std::env::var_os("SHELLFOX_NATIVE_TEST_SUPERVISOR")
            .expect("explicit separately built shellfox-runtime-fixture"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let mut pty = PtyConfig::new(std::fs::canonicalize(&root).unwrap());
    pty.supervisor = Some(executable.clone());
    pty.command = Some(PtyCommand {
        program: executable.into_os_string(),
        args: vec!["--child-echo".into()],
    });
    let native = shellfox_runtime::Native0RuntimeConfig::new(
        RestorationConfig {
            runtime_image_storage_limit: 10_000_000,
            runtime_resize_pull_scrollback: false,
        },
        SnapshotLimits::default(),
    )
    .unwrap();
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    // Sender comes from actual locally verified engine capability BEFORE capture,
    // then from PAIR selection at the receiver, never recovery.metadata.
    let engine = Runtime::native0_engine_identity().unwrap();
    let sender = m::Native0Codec::new(
        m::EngineIdentity::try_from(String::from_utf8(engine.as_bytes().to_vec()).unwrap())
            .unwrap(),
    );
    let runtime = Runtime::start_remote_eligible_native0_program_with_limits_and_hooks(
        identity,
        pty,
        Default::default(),
        None,
        RuntimeHooks::default(),
        native,
    )
    .unwrap();
    let server = Arc::new(Mutex::new(Server {
        runtime,
        book: None,
        lease: None,
        selection: None,
        sender,
        next: 1,
        acks: 0,
    }));
    eprintln!("BOUNDARY_OWNED_SCRATCH {}", root.display());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        executor.block_on(exercise(server.clone(), identity))
    }));
    // Retain the original Runtime outside a panicking future until actual Closed.
    server
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .close_owned();
    drop(server);
    if let Err(panic) = outcome {
        eprintln!("BOUNDARY_FAILED_FIXTURE_RETAINED {}", root.display());
        std::panic::resume_unwind(panic);
    }
    std::fs::remove_dir_all(root).unwrap();
}
async fn exercise(server: Arc<Mutex<Server>>, identity: Identity) {
    let mut receiver = ManagerRecovery::native0(
        AuthenticatedPeerId::new("owned-fixture-peer").unwrap(),
        id(2),
        Transport(server.clone()),
        80,
        24,
        SnapshotLimits::default(),
    )
    .unwrap();
    receiver
        .discover(shellfox_client::manager_v2::FallbackPolicy::Never)
        .await
        .unwrap();
    receiver.pair(id(3)).await.unwrap();
    receiver
        .attach(m::AttachTerminal {
            tab_id: identity.tab_id.to_string().parse().unwrap(),
            resume: None,
            control: m::ControlRequest::IfFree,
        })
        .await
        .unwrap();
    assert!(receiver.acknowledge_consumed().await.is_err());
    assert_eq!(server.lock().unwrap().acks, 0);
    let lease = server.lock().unwrap().lease.unwrap();
    assert!(server
        .lock()
        .unwrap()
        .request(Command::CheckControllerReady { lease })
        .is_err());
    let canceled = receiver.prepare_pending().unwrap();
    receiver.cancel_pending();
    assert!(receiver.commit_prepared(canceled).is_err());
    assert!(receiver.acknowledge_consumed().await.is_err());
    assert_eq!(server.lock().unwrap().acks, 0);
    assert!(server
        .lock()
        .unwrap()
        .request(Command::CheckControllerReady { lease })
        .is_err());
    // Explicit fresh recovery on the SAME known lease; no unknown-grant retry.
    receiver
        .attach(m::AttachTerminal {
            tab_id: identity.tab_id.to_string().parse().unwrap(),
            resume: None,
            control: m::ControlRequest::IfFree,
        })
        .await
        .unwrap();
    let prepared = receiver.prepare_pending().unwrap();
    assert!(receiver.acknowledge_consumed().await.is_err());
    receiver.commit_prepared(prepared).unwrap();
    assert!(matches!(
        receiver.acknowledge_consumed().await.unwrap(),
        m::Reply::Success(_)
    ));
    assert_eq!(server.lock().unwrap().acks, 1);
    assert!(matches!(
        server
            .lock()
            .unwrap()
            .request(Command::CheckControllerReady { lease })
            .unwrap(),
        Response::Applied
    ));
    let Response::Input(receipt) = server
        .lock()
        .unwrap()
        .request(Command::InteractiveBytes {
            lease,
            bytes: b"delta\r".to_vec(),
        })
        .unwrap()
    else {
        panic!("actual input receipt")
    };
    assert_eq!(receipt.accepted_bytes, 6);
    assert_eq!(receipt.kind, shellfox_runtime::InputKind::Raw);
    eprintln!("BOUNDARY_ACTUAL_INPUT_RECEIPT {receipt:?}");
    let end = Instant::now() + Duration::from_secs(15);
    let mut observed = Vec::new();
    loop {
        let m::Reply::Success(reply) = receiver.poll().await.unwrap() else {
            panic!("actual original-origin poll")
        };
        let mut data = false;
        for event in &reply.events {
            if let m::HostEvent::Terminal {
                event: m::Event::Data { data: bytes, .. },
            } = event
            {
                let bytes =
                    shellfox_protocol::observer_v2::decode_bytes(bytes, m::MAX_DATA_BYTES).unwrap();
                assert!(observed.len() + bytes.len() <= 65536);
                observed.extend(bytes);
                data = true;
            }
        }
        if data {
            receiver.pump().unwrap();
            assert!(matches!(
                receiver.acknowledge_consumed().await.unwrap(),
                m::Reply::Success(_)
            ));
        }
        if observed
            .windows(b"RUNTIME_ECHO=delta".len())
            .any(|part| part == b"RUNTIME_ECHO=delta")
        {
            break;
        }
        assert!(
            Instant::now() < end,
            "original child echo must arrive through SDK poll"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(server.lock().unwrap().acks >= 2);
    eprintln!(
        "BOUNDARY_ACTUAL_CHILD_DELTA {}",
        String::from_utf8_lossy(&observed)
    );
    receiver.close().unwrap();
}
