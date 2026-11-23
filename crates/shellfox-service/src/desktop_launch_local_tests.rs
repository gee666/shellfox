use super::*;
use shellfox_protocol::{LocalRequest, Validate};

#[test]
fn launch_titles_match_directory_basename_root_and_utf16_bound_without_settings_inference() {
    use super::super::service::launch_title;
    assert_eq!(launch_title("/"), "/");
    assert_eq!(launch_title("C:\\"), "C:\\");
    assert_eq!(launch_title("/work/back\\slash"), "back\\slash");
    assert_eq!(launch_title("C:\\work\\some folder\\"), "some folder");
    let name = "a".repeat(199) + "😀";
    assert_eq!(launch_title(&format!("/work/{name}")), "a".repeat(199));
}

#[test]
fn exact_shared_private_namespace_and_three_typed_commands_validate() {
    assert_eq!(
        <DesktopLaunchRequest as LocalRequest>::METHOD,
        shellfox_protocol::DESKTOP_LAUNCH_NAMESPACE
    );
    let host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let request = intent();
    let _registration = router.register(owner, &host).unwrap();
    for command in [
        DesktopLaunchRequest::Discover {},
        DesktopLaunchRequest::Submit {
            owner,
            intent: request.clone(),
            submission: LaunchRequestKind::Fresh,
        },
        DesktopLaunchRequest::Receipt {
            owner,
            request_id: request.request_id,
        },
    ] {
        command.validate().unwrap();
        let bytes = serde_json::to_vec(&command).unwrap();
        let decoded: DesktopLaunchRequest = serde_json::from_slice(&bytes).unwrap();
        let reply = router.handle_local(decoded, &host).unwrap();
        reply.validate().unwrap();
    }
    assert_eq!(host.creations, 0); // DTO admission is not metadata creation/focus.
}

#[test]
fn private_submit_to_headless_owner_is_rejected_without_admission_or_recreation() {
    let host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let request = intent();
    for submission in [LaunchRequestKind::Fresh, LaunchRequestKind::Known] {
        assert_eq!(
            router.handle_local(
                DesktopLaunchRequest::Submit {
                    owner,
                    intent: request.clone(),
                    submission
                },
                &host
            ),
            Err(LaunchError::NoDesktop)
        );
    }
    assert!(router.entries.is_empty());
    assert_eq!(host.creations, 0);
    assert_eq!(
        router.receipt(owner, request.request_id, &host),
        Err(LaunchError::ReceiptUnavailable)
    );
}

#[test]
fn native_registration_claim_ack_and_arbitrary_payloads_are_not_private_commands() {
    for operation in [
        "register",
        "claim",
        "ack",
        "complete",
        "stop",
        "settings",
        "experimental.anything",
    ] {
        let raw = serde_json::json!({"operation": operation});
        assert!(serde_json::from_value::<DesktopLaunchRequest>(raw).is_err());
    }
    for raw in [
        serde_json::json!({"operation":"discover","register":true}),
        serde_json::json!({"operation":"discover","peerId":"remote"}),
        serde_json::json!({"operation":"receipt","owner":"not-a-uuid","requestId":"bad"}),
    ] {
        assert!(serde_json::from_value::<DesktopLaunchRequest>(raw).is_err());
    }
}

#[test]
fn invalid_paths_nil_fences_conflicting_ids_and_extra_fields_are_rejected() {
    let host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    for cwd in ["relative", "", "https://remote/path", "C:\\work\nother"] {
        let request = DesktopLaunchIntent::new(
            LaunchAction::NewSession { cwd: cwd.into() },
            LaunchDisposition::ExistingWindow,
        );
        assert_eq!(
            router.enqueue(owner, request, &host),
            Err(LaunchError::Invalid)
        );
    }
    let nil = serde_json::json!({"operation":"submit", "owner":"00000000-0000-0000-0000-000000000000",
        "intent":intent(), "submission":"fresh"});
    let request: DesktopLaunchRequest = serde_json::from_value(nil).unwrap();
    assert!(request.validate().is_err());
    let mut extra = serde_json::to_value(intent()).unwrap();
    extra["takeover"] = true.into();
    assert!(serde_json::from_value::<DesktopLaunchIntent>(extra).is_err());
    assert!(router.entries.is_empty());
}

#[test]
fn known_submit_uses_only_durable_resolution_not_new_creation() {
    let mut host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let request = intent();
    let mut registration = router.register(owner, &host).unwrap();
    router
        .handle_local(
            DesktopLaunchRequest::Submit {
                owner,
                intent: request.clone(),
                submission: LaunchRequestKind::Known,
            },
            &host,
        )
        .unwrap();
    assert_eq!(
        router
            .dispatch_one(owner, &mut host)
            .unwrap()
            .unwrap()
            .state,
        LaunchState::Failed(LaunchError::ReceiptUnavailable)
    );
    assert!(registration.receiver.try_recv().is_err());
    assert_eq!(host.creations, 0);
    let reply = router
        .handle_local(
            DesktopLaunchRequest::Receipt {
                owner,
                request_id: request.request_id,
            },
            &host,
        )
        .unwrap();
    assert_eq!(
        reply,
        DesktopLaunchReply::Receipt(LaunchReceipt {
            request_id: request.request_id,
            state: LaunchState::Failed(LaunchError::ReceiptUnavailable)
        })
    );
}

#[test]
fn builds_match_only_their_own_build_id() {
    let stamp = crate::desktop_launch::build_stamp("0.10.0+abc");
    assert_eq!(crate::desktop_launch::build_stamp("0.10.0+abc"), stamp);
    assert_ne!(crate::desktop_launch::build_stamp("0.10.0+abc-dirty"), stamp);
    assert_ne!(crate::desktop_launch::build_stamp("0.10.1+abc"), stamp);
    assert_ne!(stamp, 0, "0 is reserved for hosts that report no build");
}
