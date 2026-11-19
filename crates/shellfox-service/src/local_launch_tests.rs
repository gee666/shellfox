use super::*;
use std::{cell::RefCell, rc::Rc};
fn owner(value: u8) -> LaunchOwnerFence {
    format!("{value:08x}-1111-4111-8111-111111111111")
        .parse()
        .unwrap()
}
fn intent() -> DesktopLaunchIntent {
    DesktopLaunchIntent::new(LaunchAction::Show {}, LaunchDisposition::ExistingWindow)
}
fn run<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn pinned_owner_replacement_rejects_before_submit_without_rediscovery() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    let result = run(launch_existing_request(
        intent(),
        LaunchRequestKind::Known,
        Some(owner(1)),
        move |request| {
            seen.borrow_mut().push(request);
            async {
                Ok(DesktopLaunchReply::Route(LaunchRouteInfo {
                    owner: owner(2),
                    desktop_registered: true,
                }))
            }
        },
    ));
    assert_eq!(
        result.unwrap_err().downcast_ref::<LaunchError>(),
        Some(&LaunchError::StaleOwner)
    );
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert!(matches!(calls[0], DesktopLaunchRequest::Discover {}));
}

#[test]
fn pinned_receipt_polling_never_adopts_replacement_or_replays_known_intent() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    let input = intent();
    let id = input.request_id;
    let result = run(launch_existing_request(
        input,
        LaunchRequestKind::Known,
        Some(owner(1)),
        move |request| {
            let reply = match &request {
                DesktopLaunchRequest::Discover {} => {
                    Ok(DesktopLaunchReply::Route(LaunchRouteInfo {
                        owner: owner(1),
                        desktop_registered: true,
                    }))
                }
                DesktopLaunchRequest::Submit {
                    owner: fence,
                    intent,
                    submission,
                } => {
                    assert_eq!(*fence, owner(1));
                    assert_eq!(intent.request_id, id);
                    assert_eq!(*submission, LaunchRequestKind::Known);
                    Ok(DesktopLaunchReply::Receipt(LaunchReceipt {
                        request_id: id,
                        state: LaunchState::Queued,
                    }))
                }
                DesktopLaunchRequest::Receipt {
                    owner: fence,
                    request_id,
                } => {
                    assert_eq!(*fence, owner(1));
                    assert_eq!(*request_id, id);
                    Err(anyhow::Error::from(LaunchError::StaleOwner))
                }
                other => panic!("Unexpected launch request: {other:?}"),
            };
            seen.borrow_mut().push(request);
            async move { reply }
        },
    ));
    assert_eq!(
        result.unwrap_err().downcast_ref::<LaunchError>(),
        Some(&LaunchError::StaleOwner)
    );
    let calls = calls.borrow();
    assert_eq!(calls.len(), 3);
    assert!(matches!(calls[0], DesktopLaunchRequest::Discover {}));
    assert!(matches!(calls[1], DesktopLaunchRequest::Submit { .. }));
    assert!(matches!(calls[2], DesktopLaunchRequest::Receipt { .. }));
}

#[test]
fn owner_without_desktop_never_submits_or_starts_anything() {
    let result = run(launch_existing_request(
        intent(),
        LaunchRequestKind::Known,
        Some(owner(1)),
        |request| {
            assert!(matches!(request, DesktopLaunchRequest::Discover {}));
            async {
                Ok(DesktopLaunchReply::Route(LaunchRouteInfo {
                    owner: owner(1),
                    desktop_registered: false,
                }))
            }
        },
    ));
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("no desktop is registered")
    );
}

#[test]
fn queued_is_not_success_and_only_correlated_handled_completes_the_pinned_handoff() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    let input = intent();
    let id = input.request_id;
    let result = run(launch_existing_request(
        input,
        LaunchRequestKind::Known,
        Some(owner(1)),
        move |request| {
            let reply = match &request {
                DesktopLaunchRequest::Discover {} => DesktopLaunchReply::Route(LaunchRouteInfo {
                    owner: owner(1),
                    desktop_registered: true,
                }),
                DesktopLaunchRequest::Submit { owner: fence, .. } => {
                    assert_eq!(*fence, owner(1));
                    DesktopLaunchReply::Receipt(LaunchReceipt {
                        request_id: id,
                        state: LaunchState::Queued,
                    })
                }
                DesktopLaunchRequest::Receipt {
                    owner: fence,
                    request_id,
                } => {
                    assert_eq!(*fence, owner(1));
                    assert_eq!(*request_id, id);
                    let reads = seen
                        .borrow()
                        .iter()
                        .filter(|call| matches!(call, DesktopLaunchRequest::Receipt { .. }))
                        .count();
                    DesktopLaunchReply::Receipt(LaunchReceipt {
                        request_id: id,
                        state: if reads == 0 {
                            LaunchState::Applying
                        } else {
                            LaunchState::Handled
                        },
                    })
                }
                other => panic!("Unexpected launch request: {other:?}"),
            };
            seen.borrow_mut().push(request);
            async move { Ok(reply) }
        },
    ))
    .unwrap();
    assert_eq!(result.state, LaunchState::Handled);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, DesktopLaunchRequest::Discover {}))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, DesktopLaunchRequest::Submit { .. }))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, DesktopLaunchRequest::Receipt { .. }))
            .count(),
        2
    );
}

#[test]
fn unpinned_cli_discovery_preserves_fresh_submission_and_validates_receipt_id() {
    let input = intent();
    let id = input.request_id;
    let result = run(launch_existing_request(
        input,
        LaunchRequestKind::Fresh,
        None,
        |request| async move {
            match request {
                DesktopLaunchRequest::Discover {} => {
                    Ok(DesktopLaunchReply::Route(LaunchRouteInfo {
                        owner: owner(2),
                        desktop_registered: true,
                    }))
                }
                DesktopLaunchRequest::Submit {
                    owner: fence,
                    intent,
                    submission,
                } => {
                    assert_eq!(fence, owner(2));
                    assert_eq!(intent.request_id, id);
                    assert_eq!(submission, LaunchRequestKind::Fresh);
                    Ok(DesktopLaunchReply::Receipt(LaunchReceipt {
                        request_id: shellfox_core::SessionId::new(),
                        state: LaunchState::Handled,
                    }))
                }
                DesktopLaunchRequest::Receipt { .. } => {
                    panic!("A mismatched handled receipt cannot authorize polling or replay")
                }
                other => panic!("Unexpected launch request: {other:?}"),
            }
        },
    ));
    assert_eq!(
        result.unwrap_err().downcast_ref::<LaunchError>(),
        Some(&LaunchError::Invalid)
    );
}
