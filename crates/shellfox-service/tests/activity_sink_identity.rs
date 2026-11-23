use shellfox_core::{Generation, SessionId, TabId};
use shellfox_host::CapturedSettlement;
use shellfox_runtime::{Identity, InputKind, InputReceipt, RequestId, Response, RuntimeHooks};
use shellfox_service::activity::integration::{InputAdmission, InspectionScope, OwnerActivity};
use std::sync::Arc;

#[test]
fn program_rewatch_keeps_the_exact_hook_sink_and_admission_book_and_rejection_cannot_orphan_it() {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let mut owner = OwnerActivity::new();
    owner
        .watch(
            identity,
            "unsupported-owned-shell".into(),
            InspectionScope::Unsupported,
        )
        .unwrap();
    let original = owner.live_output_sink(identity).unwrap();
    let hooks = RuntimeHooks {
        live_output: Some(original.clone()),
    };
    let captured = CapturedSettlement::Captured(Some("2026-01-01T00:00:00.000Z".into()));
    original
        .register_input(
            identity,
            RequestId(1),
            InputAdmission::raw(b"x", true, true, captured.clone()).unwrap(),
        )
        .unwrap();
    // The real SSH starter refines inspection for this SAME generation. No
    // Runtime, PTY, process, authentication or control lease is created here.
    owner
        .watch(identity, "C:\\Tools\\ssh.exe".into(), InspectionScope::Ssh)
        .unwrap();
    let current = owner.live_output_sink(identity).unwrap();
    assert!(Arc::ptr_eq(&original, &current));
    let hook = hooks.live_output.as_ref().unwrap();
    let at = owner.clock().instant_at(100).unwrap();
    hook.before_input(identity, RequestId(1), InputKind::Raw, b"x", at)
        .unwrap();
    hook.output(identity, 1, b"echo", at).unwrap();
    let received = Ok(Response::Input(InputReceipt {
        accepted_bytes: 1,
        kind: InputKind::Raw,
    }));
    let accepted = current
        .finish_input(identity, RequestId(1), &received)
        .unwrap()
        .unwrap();
    assert!(accepted.activity.touch_session);
    assert_eq!(accepted.admission.captured_settlement, captured);
    let output = owner.take_live_updates().remove(0);
    assert_eq!(output.last_sequence, 1);
    assert_eq!(output.output_at_ms, Some(100));
    assert_eq!(
        output.meaningful_at_ms, None,
        "pre-write echo used the same policy mutex"
    );
    assert!(output.available);
    assert!(owner
        .watch(identity, String::new(), InspectionScope::Ssh)
        .is_err());
    assert!(Arc::ptr_eq(
        &original,
        &owner.live_output_sink(identity).unwrap()
    ));
}
