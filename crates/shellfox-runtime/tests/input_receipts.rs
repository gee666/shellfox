#![cfg(windows)]
mod support;
use shellfox_ghostty_vt::{
    Key, KeyAction, KeyEvent, Modifiers, MouseAction, MouseEvent, MouseGeometry,
};
use shellfox_runtime::*;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};
use support::Harness;
#[derive(Default)]
struct ObserveInput {
    calls: AtomicU64,
    request: AtomicU64,
    bytes: AtomicU64,
    single_byte: AtomicU64,
    fail: AtomicBool,
}
impl LiveOutputSink for ObserveInput {
    fn before_input(
        &self,
        _: Identity,
        request: RequestId,
        _: InputKind,
        encoded: &[u8],
        _: Instant,
    ) -> Result<(), LiveOutputFailure> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.request.store(request.0, Ordering::Relaxed);
        self.bytes.store(encoded.len() as u64, Ordering::Relaxed);
        self.single_byte.store(match encoded { [byte] => u64::from(*byte), _ => u64::MAX }, Ordering::Relaxed);
        if self.fail.load(Ordering::Relaxed) {
            Err(LiveOutputFailure)
        } else {
            Ok(())
        }
    }
    fn output(&self, _: Identity, _: u64, _: &[u8], _: Instant) -> Result<(), LiveOutputFailure> {
        Ok(())
    }
}
fn receipt(response: Response) -> InputReceipt {
    match response {
        Response::Input(receipt) => receipt,
        response => panic!("input receipt expected, got {response:?}"),
    }
}
fn host(mode: &str) -> (Harness, Arc<ObserveInput>) {
    let observer = Arc::new(ObserveInput::default());
    let host = Harness::start_with_hooks(
        mode,
        RuntimeHooks {
            live_output: Some(observer.clone()),
        },
    );
    (host, observer)
}
#[test]
fn default_runtime_returns_typed_zero_receipt_without_an_activity_supplier() {
    let mut host = Harness::start("--child-idle");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_IDLE_READY"));
    assert_eq!(
        receipt(
            host.request(Command::RawInput {
                lease: host.lease,
                bytes: Vec::new()
            })
            .unwrap()
        )
        .accepted_bytes,
        0
    );
    assert_eq!(
        host.runtime.live_output_status(),
        LiveOutputStatus::Disabled
    );
}
#[test]
fn ignored_encodings_return_input_zero_and_skip_both_prewrite_hook_and_actual_write() {
    let (mut host, observer) = host("--child-idle");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_IDLE_READY"));
    for command in [
        Command::Key {
            lease: host.lease,
            event: KeyEvent::default(),
        },
        Command::Key {
            lease: host.lease,
            event: KeyEvent {
                key: Key::ArrowUp,
                action: KeyAction::Release,
                ..Default::default()
            },
        },
        Command::Paste {
            lease: host.lease,
            text: String::new(),
        },
        Command::RawInput {
            lease: host.lease,
            bytes: Vec::new(),
        },
        Command::Mouse {
            lease: host.lease,
            event: MouseEvent {
                action: MouseAction::Motion,
                button: None,
                modifiers: Modifiers::default(),
                x: 1.0,
                y: 1.0,
                any_button_pressed: false,
            },
            geometry: MouseGeometry {
                cell_width: 8,
                cell_height: 18,
            },
        },
    ] {
        let kind = match &command {
            Command::Key { event, .. } => InputKind::Key {
                action: event.action,
                has_committed_text: false,
            },
            Command::Paste { .. } => InputKind::Paste { nonempty: false },
            Command::Mouse { .. } => InputKind::Mouse {
                action: MouseAction::Motion,
                button: None,
                any_button_pressed: false,
            },
            Command::RawInput { .. } => InputKind::Raw,
            _ => unreachable!("controlled input command"),
        };
        let result = receipt(host.request(command).unwrap());
        assert_eq!(result.accepted_bytes, 0);
        assert_eq!(result.kind, kind);
    }
    assert_eq!(observer.calls.load(Ordering::Relaxed), 0);
    assert_eq!(host.runtime.phase(), Phase::Active);
}
#[test]
fn xterm_nul_reaches_the_conpty_write_boundary_as_one_byte() {
    let (mut host, observer) = host("--child-idle");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_IDLE_READY"));
    // GPUI may carry Shift+2 as an already shifted key on Windows.
    for (key, shift) in [(' ', false), ('2', true), ('@', false)] {
        let response = host.request(Command::Key {
            lease: host.lease,
            event: KeyEvent {
                key: Key::Character(key),
                modifiers: Modifiers { control: true, shift, ..Default::default() },
                ..Default::default()
            },
        }).unwrap();
        assert_eq!(receipt(response).accepted_bytes, 1, "{key:?}");
        assert_eq!(observer.bytes.load(Ordering::Relaxed), 1);
        assert_eq!(observer.single_byte.load(Ordering::Relaxed), 0);
    }
    assert_eq!(observer.calls.load(Ordering::Relaxed), 3);
}

#[test]
fn native_key_receipt_reports_action_and_text_presence_without_retaining_the_text() {
    let (mut host, observer) = host("--child-idle");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_IDLE_READY"));
    let result = receipt(
        host.request(Command::Key {
            lease: host.lease,
            event: KeyEvent {
                key: Key::Character('k'),
                text: Some("k".into()),
                ..Default::default()
            },
        })
        .unwrap(),
    );
    assert!(result.accepted_bytes > 0);
    assert_eq!(
        result.kind,
        InputKind::Key {
            action: KeyAction::Press,
            has_committed_text: true
        }
    );
    assert_eq!(observer.calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        observer.bytes.load(Ordering::Relaxed),
        u64::from(result.accepted_bytes)
    );
}
#[test]
fn confirmed_write_receipt_keeps_request_identity_and_contains_no_input_text() {
    let (mut host, observer) = host("--child-echo");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_READY"));
    let admitted_request = host.next;
    let input = b"not-a-real-password\r";
    let result = receipt(
        host.request(Command::RawInput {
            lease: host.lease,
            bytes: input.to_vec(),
        })
        .unwrap(),
    );
    assert_eq!(result.accepted_bytes, input.len() as u32);
    assert_eq!(result.kind, InputKind::Raw);
    assert_eq!(observer.calls.load(Ordering::Relaxed), 1);
    assert_eq!(observer.request.load(Ordering::Relaxed), admitted_request);
    assert_eq!(observer.bytes.load(Ordering::Relaxed), input.len() as u64);
    assert!(!format!("{result:?}").contains("not-a-real-password"));
    host.until(|host| host.exited);
}
#[test]
fn encoded_paste_overhead_is_validated_before_prewrite_and_does_not_close_the_pty() {
    let (mut host, observer) = host("--child-bracketed-idle");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_BRACKETED_READY"));
    let result = receipt(
        host.request(Command::Paste {
            lease: host.lease,
            text: "x".into(),
        })
        .unwrap(),
    );
    assert_eq!(result.accepted_bytes, 13);
    assert_eq!(result.kind, InputKind::Paste { nonempty: true });
    let before = observer.calls.load(Ordering::Relaxed);
    let error = host
        .request(Command::Paste {
            lease: host.lease,
            text: "x".repeat(MAX_INPUT_BYTES),
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Validation);
    assert_eq!(observer.calls.load(Ordering::Relaxed), before);
    assert_eq!(host.runtime.phase(), Phase::Active);
}
#[test]
fn failing_prewrite_observer_cannot_erase_a_real_write_or_kill_the_owned_pty() {
    let (mut host, observer) = host("--child-echo");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_READY"));
    observer.fail.store(true, Ordering::Relaxed);
    let result = receipt(
        host.request(Command::RawInput {
            lease: host.lease,
            bytes: b"still-written\r".to_vec(),
        })
        .unwrap(),
    );
    assert_eq!(result.accepted_bytes, 14);
    assert_eq!(
        host.runtime.live_output_status(),
        LiveOutputStatus::Unavailable
    );
    host.until(|host| host.exited);
    assert!(String::from_utf8_lossy(&host.output).contains("RUNTIME_ECHO=still-written"));
}
#[test]
fn authoritative_parser_replies_never_enter_the_user_prewrite_hook() {
    let (mut host, observer) = host("--child-query");
    host.until(|host| host.exited);
    assert!(String::from_utf8_lossy(&host.output).contains("RUNTIME_QUERY_FINAL"));
    assert_eq!(observer.calls.load(Ordering::Relaxed), 0);
}
