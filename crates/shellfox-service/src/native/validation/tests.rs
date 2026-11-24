use super::*;
use shellfox_ghostty_vt::{key::KeyEvent, Modifiers, MouseAction};

fn action(command: NativeCommand) -> NativeAction {
    let input = matches!(
        &command,
        NativeCommand::Key(_)
            | NativeCommand::Keys(_)
            | NativeCommand::Paste(_)
            | NativeCommand::Mouse { .. }
    )
    .then_some(NativeInputContext {
        captured_settlement: shellfox_host::CapturedSettlement::Captured(None),
        user_initiated: true,
        hint: true,
    });
    NativeAction::Terminal {
        tab: TabId::new(),
        attachment: Attachment {
            controller: 1,
            epoch: 1,
            generation: Generation::new(),
        },
        command,
        input,
    }
}
#[test]
fn admission_rejects_large_or_invalid_input_before_queueing() {
    assert!(action(NativeCommand::Paste("x".repeat(65537)))
        .validate()
        .is_err());
    assert!(action(NativeCommand::Key(KeyEvent::text("\0")))
        .validate()
        .is_err());
    assert!(action(NativeCommand::Key(KeyEvent::text("é🦊")))
        .validate()
        .is_ok());
    let keys = |count, text: &str| NativeCommand::Keys(vec![KeyEvent::text(text); count]);
    assert!(action(keys(256, "ab")).validate().is_ok());
    assert!(action(keys(0, "a")).validate().is_err());
    assert!(action(keys(257, "a")).validate().is_err());
    assert!(action(keys(2, "\0")).validate().is_err());
    assert!(action(keys(2, &"x".repeat(40_000))).validate().is_err());
    assert!(action(NativeCommand::Resize { cols: 0, rows: 24 })
        .validate()
        .is_err());
    assert!(action(NativeCommand::Resize {
        cols: 160,
        rows: 48
    })
    .validate()
    .is_ok());
    assert!(action(NativeCommand::Mouse {
        event: MouseEvent {
            action: MouseAction::Motion,
            button: None,
            modifiers: Modifiers::default(),
            x: f32::NAN,
            y: 0.,
            any_button_pressed: false
        },
        geometry: MouseGeometry {
            cell_width: 8,
            cell_height: 16
        }
    })
    .validate()
    .is_err());
}
#[test]
fn input_context_must_be_explicit_captured_and_only_input_carries_it() {
    let NativeAction::Terminal {
        tab,
        attachment,
        command,
        ..
    } = action(NativeCommand::Paste("x".into()))
    else {
        unreachable!()
    };
    assert!(NativeAction::Terminal {
        tab,
        attachment: attachment.clone(),
        command: command.clone(),
        input: None
    }
    .validate()
    .is_err());
    let legacy = NativeInputContext {
        captured_settlement: shellfox_host::CapturedSettlement::Legacy,
        user_initiated: true,
        hint: true,
    };
    assert!(NativeAction::Terminal {
        tab,
        attachment: attachment.clone(),
        command,
        input: Some(legacy)
    }
    .validate()
    .is_err());
    let captured = NativeInputContext {
        captured_settlement: shellfox_host::CapturedSettlement::Captured(None),
        user_initiated: true,
        hint: false,
    };
    assert!(NativeAction::Terminal {
        tab,
        attachment,
        command: NativeCommand::Copy,
        input: Some(captured)
    }
    .validate()
    .is_err());
}

#[test]
fn metadata_aggregate_limit_is_checked_without_building_a_json_buffer() {
    let env = (0..200)
        .map(|index| shellfox_core::EnvVar {
            name: format!("VARIABLE_{index}"),
            value: "x".repeat(6000),
        })
        .collect();
    let request = NativeAction::Metadata(HostRequest::SetTerminalEnv { env });
    assert!(request.validate().is_err());
    let request = NativeAction::Metadata(HostRequest::RenameSession {
        session_id: SessionId::new(),
        title: "\0".into(),
    });
    assert!(request.validate().is_err());
}
#[test]
fn only_a_full_unsent_action_is_retryable_and_error_display_hides_payloads() {
    let error = NativeAdmissionError::Full(Box::new(action(NativeCommand::Paste(
        "private-input".into(),
    ))));
    assert!(!error.to_string().contains("private-input"));
    assert!(!format!("{error:?}").contains("private-input"));
    assert!(matches!(
        error.retry(),
        Some(NativeAction::Terminal {
            command: NativeCommand::Paste(_),
            ..
        })
    ));
    let error = NativeAdmissionError::Stopped(Box::new(NativeAction::DetachAll));
    assert!(error.retry().is_none());
    assert!(NativeAdmissionError::Invalid("bad action".into())
        .retry()
        .is_none());
}
