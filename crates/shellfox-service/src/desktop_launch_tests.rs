use super::*;
use std::collections::BTreeSet;
#[path = "desktop_launch_limits_tests.rs"]
mod limits;
#[path = "desktop_launch_local_tests.rs"]
mod local_tests;

#[derive(Default)]
struct HostFake {
    lost: bool,
    lose_during_resolve: bool,
    resolutions: usize,
    creations: usize,
    requests: BTreeMap<SessionId, SessionId>,
    sessions: BTreeSet<SessionId>,
    global_selection: Option<SessionId>,
}
impl LaunchAuthority for HostFake {
    fn check_owned(&self) -> Result<(), LaunchError> {
        if self.lost {
            Err(LaunchError::StaleOwner)
        } else {
            Ok(())
        }
    }
}
impl LaunchSessionResolver for HostFake {
    fn resolve(
        &mut self,
        intent: &DesktopLaunchIntent,
        kind: LaunchRequestKind,
    ) -> Result<LaunchTarget, LaunchError> {
        self.resolutions += 1;
        let target = match &intent.action {
            LaunchAction::Show {} => LaunchTarget::Show {},
            LaunchAction::OpenSession { session } => {
                if !self.sessions.contains(session) {
                    return Err(LaunchError::NotFound);
                }
                LaunchTarget::Session {
                    session: *session,
                    tab: None,
                }
            }
            LaunchAction::NewSession { .. } => {
                let session = if let Some(id) = self.requests.get(&intent.request_id) {
                    *id
                } else if kind == LaunchRequestKind::Known {
                    return Err(LaunchError::ReceiptUnavailable);
                } else {
                    let id = SessionId::new();
                    self.creations += 1;
                    self.requests.insert(intent.request_id, id);
                    self.sessions.insert(id);
                    id
                };
                if !self.sessions.contains(&session) {
                    return Err(LaunchError::ReceiptUnavailable);
                }
                LaunchTarget::Session { session, tab: None }
            }
        };
        if self.lose_during_resolve {
            self.lost = true;
        }
        Ok(target)
    }
}
fn intent() -> DesktopLaunchIntent {
    DesktopLaunchIntent::new(
        LaunchAction::NewSession {
            cwd: "/fixture/work".into(),
        },
        LaunchDisposition::ExistingWindow,
    )
}
fn delivered(
    router: &mut DesktopLaunchRouter,
    host: &mut HostFake,
    registration: &mut DesktopRegistration,
    intent: DesktopLaunchIntent,
) -> DesktopLaunchDelivery {
    let owner = router.owner();
    router.enqueue(owner, intent, host).unwrap();
    router.dispatch_one(owner, host).unwrap().unwrap();
    registration.receiver.try_recv().unwrap()
}

#[test]
fn startup_queue_deduplicates_then_resolves_and_focuses_only_one_delivery() {
    let mut host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let request = intent();
    assert_eq!(
        router.enqueue(owner, request.clone(), &host).unwrap().state,
        LaunchState::Queued
    );
    router.enqueue(owner, request.clone(), &host).unwrap();
    assert_eq!(
        router.dispatch_one(owner, &mut host),
        Err(LaunchError::NoDesktop)
    );
    assert_eq!(host.creations, 0);
    assert_eq!(host.resolutions, 0);
    let mut registration = router.register(owner, &host).unwrap();
    router.dispatch_one(owner, &mut host).unwrap();
    let delivery = registration.receiver.try_recv().unwrap();
    assert_eq!(
        router.claim(&delivery, &mut host).unwrap(),
        LaunchClaim::Apply
    );
    // An interrupted claim is idempotent for the same unique coordinator.
    assert_eq!(
        router.claim(&delivery, &mut host).unwrap(),
        LaunchClaim::Apply
    );
    router
        .complete(&delivery, LaunchHandling::Handled, &host)
        .unwrap();
    router
        .complete(&delivery, LaunchHandling::Handled, &host)
        .unwrap();
    assert_eq!(
        router.enqueue(owner, request, &host).unwrap().state,
        LaunchState::Handled
    );
    assert_eq!(
        router.claim(&delivery, &mut host).unwrap(),
        LaunchClaim::AlreadyHandled
    );
    assert_eq!(host.creations, 1);
    assert!(registration.receiver.try_recv().is_err());
    assert_eq!(router.dispatch_one(owner, &mut host).unwrap(), None);
}

#[test]
fn conflicting_request_ids_do_not_create_or_focus_again() {
    let host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let request = intent();
    router.enqueue(owner, request.clone(), &host).unwrap();
    let mut different = request.clone();
    different.disposition = LaunchDisposition::NewWindow;
    assert_eq!(
        router.enqueue(owner, different, &host),
        Err(LaunchError::Conflict)
    );
    let mut different = request;
    different.action = LaunchAction::NewSession {
        cwd: "/other".into(),
    };
    assert_eq!(
        router.enqueue(owner, different, &host),
        Err(LaunchError::Conflict)
    );
    assert_eq!(host.creations, 0);
}

#[test]
fn durable_resume_uses_original_session_not_global_selection_or_original_tab() {
    let mut host = HostFake::default();
    let mut first = DesktopLaunchRouter::new();
    let owner = first.owner();
    let mut registration = first.register(owner, &host).unwrap();
    let request = intent();
    let delivery = delivered(&mut first, &mut host, &mut registration, request.clone());
    let original = host.requests[&request.request_id];
    let other = SessionId::new();
    host.sessions.insert(other);
    host.global_selection = Some(other);
    let mut restarted = DesktopLaunchRouter::new();
    let new_owner = restarted.owner();
    assert_ne!(owner, new_owner);
    let mut next = restarted.register(new_owner, &host).unwrap();
    restarted.resume(new_owner, request.clone(), &host).unwrap();
    restarted.dispatch_one(new_owner, &mut host).unwrap();
    let resumed = next.receiver.try_recv().unwrap();
    assert_eq!(
        resumed.target,
        LaunchTarget::Session {
            session: original,
            tab: None
        }
    );
    assert_eq!(host.global_selection, Some(other));
    assert_eq!(host.creations, 1);
    assert_eq!(
        restarted.claim(&delivery, &mut host),
        Err(LaunchError::StaleOwner)
    );
    assert_eq!(
        restarted.claim(&resumed, &mut host).unwrap(),
        LaunchClaim::Apply
    );
}

#[test]
fn deleted_or_unknown_known_receipt_never_recreates_and_is_not_dispatched() {
    for deleted in [false, true] {
        let mut host = HostFake::default();
        let request = intent();
        if deleted {
            host.resolve(&request, LaunchRequestKind::Fresh).unwrap();
            host.sessions.clear();
        }
        let before = host.creations;
        let mut router = DesktopLaunchRouter::new();
        let owner = router.owner();
        let mut registration = router.register(owner, &host).unwrap();
        router.resume(owner, request.clone(), &host).unwrap();
        assert_eq!(
            router
                .dispatch_one(owner, &mut host)
                .unwrap()
                .unwrap()
                .state,
            LaunchState::Failed(LaunchError::ReceiptUnavailable)
        );
        // Changing submission mode cannot turn a known failed ID into creation.
        assert_eq!(
            router.enqueue(owner, request, &host).unwrap().state,
            LaunchState::Failed(LaunchError::ReceiptUnavailable)
        );
        assert_eq!(host.creations, before);
        assert!(registration.receiver.try_recv().is_err());
    }
}

#[test]
fn deleted_target_is_rechecked_before_ui_claim() {
    let mut host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let mut registration = router.register(owner, &host).unwrap();
    let delivery = delivered(&mut router, &mut host, &mut registration, intent());
    host.sessions.clear();
    assert_eq!(
        router.claim(&delivery, &mut host),
        Err(LaunchError::ReceiptUnavailable)
    );
    assert_eq!(
        router
            .receipt(owner, delivery.intent.request_id, &host)
            .unwrap()
            .state,
        LaunchState::Failed(LaunchError::ReceiptUnavailable)
    );
    assert_eq!(host.creations, 1);
}

#[test]
fn stale_registration_and_ownership_loss_do_not_replay_to_another_desktop() {
    let mut host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let mut old = router.register(owner, &host).unwrap();
    assert!(matches!(
        router.register(owner, &host),
        Err(LaunchError::Busy)
    ));
    let delivery = delivered(&mut router, &mut host, &mut old, intent());
    router.unregister(old.token, &host).unwrap();
    let mut new = router.register(owner, &host).unwrap();
    assert_eq!(
        router.claim(&delivery, &mut host),
        Err(LaunchError::StaleClient)
    );
    assert_eq!(
        router.complete(&delivery, LaunchHandling::Handled, &host),
        Err(LaunchError::StaleClient)
    );
    assert_eq!(
        router
            .receipt(owner, delivery.intent.request_id, &host)
            .unwrap()
            .state,
        LaunchState::Uncertain
    );
    assert!(new.receiver.try_recv().is_err());
    let fresh = delivered(&mut router, &mut host, &mut new, intent());
    host.lost = true;
    assert_eq!(
        router.claim(&fresh, &mut host),
        Err(LaunchError::StaleOwner)
    );
    assert!(matches!(
        router.register(owner, &host),
        Err(LaunchError::StaleOwner)
    ));
}

#[test]
fn ownership_loss_during_resolution_is_uncertain_not_a_second_launch() {
    let mut host = HostFake {
        lose_during_resolve: true,
        ..Default::default()
    };
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let mut registration = router.register(owner, &host).unwrap();
    let request = intent();
    router.enqueue(owner, request.clone(), &host).unwrap();
    assert_eq!(
        router.dispatch_one(owner, &mut host),
        Err(LaunchError::StaleOwner)
    );
    assert_eq!(host.creations, 1);
    assert_eq!(
        router.entries[&request.request_id].state,
        LaunchState::Uncertain
    );
    assert!(registration.receiver.try_recv().is_err());
}

#[test]
fn wsl_folders_open_in_their_distro() {
    let profile = |cwd: &str| folder_profile(cwd).map(|id| id.as_str().to_owned());
    assert_eq!(
        profile(r"\\wsl.localhost\Ubuntu-24.04\home\a b").as_deref(),
        Some("wsl:Ubuntu-24.04")
    );
    assert_eq!(profile(r"\\WSL$\Debian").as_deref(), Some("wsl:Debian"));
    for other in [
        r"C:\work",
        r"\\server\share\x",
        r"\\wsl.localhost\",
        "/home/a",
    ] {
        assert_eq!(profile(other), None, "{other}");
    }
}
