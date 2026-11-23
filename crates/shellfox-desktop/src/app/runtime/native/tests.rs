use super::*;
use shellfox_core::Generation;
use shellfox_ghostty_vt::key::{KeyAction, KeyEvent, KeyModifiers, PhysicalKey};
fn access() -> (Access, Lease, TabId) {
    let lease = Lease {
        controller: 8,
        epoch: 2,
    };
    (
        Access::Control(Attachment {
            controller: lease.controller,
            epoch: lease.epoch,
            generation: Generation::new(),
        }),
        lease,
        TabId::new(),
    )
}
#[test]
fn controller_input_uses_checked_service_attachment_and_ghostty_key_data() {
    let (access, lease, tab) = access();
    let action = access
        .action(
            tab,
            Command::Key {
                lease,
                event: KeyEvent::press(PhysicalKey::KeyC, KeyModifiers::CONTROL),
            },
        )
        .unwrap();
    assert!(
        matches!(action, NativeAction::Terminal { attachment, command: NativeCommand::Key(event), .. }
        if attachment.controller == lease.controller && event.modifiers == KeyModifiers::CONTROL && event.physical == PhysicalKey::KeyC)
    );
    assert!(
        access
            .action(
                tab,
                Command::Paste {
                    lease: Lease { epoch: 3, ..lease },
                    text: "not accepted".into()
                }
            )
            .is_err()
    );
}
#[test]
fn closing_a_view_detaches_and_never_requests_pty_cleanup() {
    let (access, _, tab) = access();
    assert!(
        matches!(access.action(tab, Command::RetryClose).unwrap(), NativeAction::Detach { tab: id, .. } if id == tab)
    );
}
#[test]
fn observer_does_not_claim_or_send_control_input() {
    let attachment = NativeObserverAttachment {
        viewer: 9,
        epoch: 4,
        generation: Generation::new(),
    };
    let access = Access::Observer(attachment.clone());
    let tab = TabId::new();
    assert!(access.lease().is_none());
    assert!(
        access
            .action(
                tab,
                Command::Paste {
                    lease: Lease {
                        controller: 9,
                        epoch: 4
                    },
                    text: "forbidden".into()
                }
            )
            .is_err()
    );
    assert!(
        matches!(access.action(tab, Command::RetryClose).unwrap(), NativeAction::Unobserve { attachment: a, .. } if a == attachment)
    );
    assert!(matches!(
        access
            .action(
                tab,
                Command::ObserverRecover {
                    observer: shellfox_runtime::ObserverLease {
                        viewer: 9,
                        epoch: 4
                    },
                    capability: shellfox_runtime::AnsiRecoveryCapability::VisibleScreen
                }
            )
            .unwrap(),
        NativeAction::RecoverObserver { .. }
    ));
}
#[test]
fn observer_presentation_is_private_and_does_not_require_input_context_or_claim() {
    let attachment = NativeObserverAttachment {
        viewer: 19,
        epoch: 4,
        generation: Generation::new(),
    };
    let access = Access::Observer(attachment.clone());
    let tab = TabId::new();
    for action in [
        shellfox_runtime::PresentationAction::Scroll { delta: -2 },
        shellfox_runtime::PresentationAction::Selection { selection: None },
        shellfox_runtime::PresentationAction::Copy,
        shellfox_runtime::PresentationAction::Hyperlink {
            point: shellfox_ghostty_vt::ViewportPoint { x: 0, y: 0 },
        },
    ] {
        let result = access
            .action_with_input(
                tab,
                Command::Presentation {
                    view: access.presentation_lease(),
                    action,
                },
                None,
            )
            .unwrap();
        assert!(
            matches!(result, NativeAction::Presentation { attachment: NativeViewAttachment::Observer(ref a), .. } if a == &attachment)
        );
    }
    assert!(
        access
            .action_with_input(
                tab,
                Command::Presentation {
                    view: shellfox_runtime::PresentationLease::Observer(
                        shellfox_runtime::ObserverLease {
                            viewer: 19,
                            epoch: 5
                        }
                    ),
                    action: shellfox_runtime::PresentationAction::Copy
                },
                None
            )
            .is_err()
    );
}

#[test]
fn external_layout_resize_forwards_exact_dimensions_as_presentation_not_pty_control() {
    let observer = Access::Observer(NativeObserverAttachment { viewer: 19, epoch: 4, generation: Generation::new() });
    for access in [access().0, observer] {
        let action = access.action_with_input(TabId::new(), Command::Presentation {
            view: access.presentation_lease(),
            action: shellfox_runtime::PresentationAction::ResizeExternal { cols: 83, rows: 29 },
        }, None).unwrap();
        assert!(matches!(action, NativeAction::Presentation {
            action: NativePresentation::ResizeExternal { cols: 83, rows: 29 }, ..
        }));
    }
}

#[test]
fn guarded_presentation_version_passes_unchanged_and_stale_is_never_retryable() {
    let access = Access::Observer(NativeObserverAttachment {
        viewer: 19,
        epoch: 4,
        generation: Generation::new(),
    });
    let action = access
        .action_with_input(
            TabId::new(),
            Command::Presentation {
                view: access.presentation_lease(),
                action: shellfox_runtime::PresentationAction::SelectionAtVersion {
                    version: 73,
                    selection: None,
                },
            },
            None,
        )
        .unwrap();
    assert!(matches!(
        action,
        NativeAction::Presentation {
            action: NativePresentation::SelectionAtVersion {
                version: 73,
                selection: None
            },
            attachment: NativeViewAttachment::Observer(_),
            ..
        }
    ));
    let stale: anyhow::Error = shellfox_runtime::RuntimeError {
        code: shellfox_runtime::ErrorCode::StalePresentation,
        message: "sample stale".into(),
    }
    .into();
    assert!(retry(stale).is_err());
}

#[test]
fn full_returns_exact_unsent_mutation_but_post_admission_failures_never_retry() {
    let (access, lease, tab) = access();
    let action = access
        .action(
            tab,
            Command::Paste {
                lease,
                text: "retained native intent".into(),
            },
        )
        .unwrap();
    let retained = retry(NativeAdmissionError::Full(Box::new(action)).into()).unwrap();
    assert!(
        matches!(retained, NativeAction::Terminal { command: NativeCommand::Paste(ref text), .. } if text == "retained native intent")
    );
    assert!(
        retry(anyhow::anyhow!(
            "Service owner queue is full; action was not accepted."
        ))
        .is_err()
    );
    assert!(
        retry(NativeAdmissionError::Stopped(Box::new(NativeAction::DetachAll)).into()).is_err()
    );
}
#[test]
fn input_receipts_including_ignored_encodings_pass_through_without_context_recapture() {
    for receipt in [
        shellfox_runtime::InputReceipt {
            accepted_bytes: 0,
            kind: shellfox_runtime::InputKind::Key {
                action: KeyAction::Release,
                has_committed_text: false,
            },
        },
        shellfox_runtime::InputReceipt {
            accepted_bytes: 8,
            kind: shellfox_runtime::InputKind::Paste { nonempty: true },
        },
        shellfox_runtime::InputReceipt {
            accepted_bytes: 0,
            kind: shellfox_runtime::InputKind::Mouse {
                action: shellfox_ghostty_vt::MouseAction::Motion,
                button: None,
                any_button_pressed: false,
            },
        },
        shellfox_runtime::InputReceipt {
            accepted_bytes: 4,
            kind: shellfox_runtime::InputKind::Raw,
        },
    ] {
        let response =
            terminal_response(NativeResponse::Terminal(Response::Input(receipt))).unwrap();
        assert!(matches!(response, Response::Input(actual) if actual == receipt));
    }
}

#[test]
fn non_input_responses_keep_their_existing_meaning() {
    assert!(matches!(
        terminal_response(NativeResponse::Applied).unwrap(),
        Response::Applied
    ));
    assert!(matches!(
        terminal_response(NativeResponse::Terminal(Response::Text(None))).unwrap(),
        Response::Text(None)
    ));
    assert!(
        terminal_response(NativeResponse::Status(
            shellfox_service::native::NativeStatus {
                active_terminals: 2
            }
        ))
        .is_err()
    );
}

#[test]
fn ui_cannot_bypass_encoding_or_grant_its_own_credit_and_control() {
    let (access, lease, tab) = access();
    for command in [
        Command::RawInput {
            lease,
            bytes: b"arbitrary".to_vec(),
        },
        Command::Ack {
            lease,
            sequence: 999,
        },
        Command::Claim {
            controller: 1,
            takeover: true,
        },
    ] {
        assert!(access.action(tab, command).is_err());
    }
}
