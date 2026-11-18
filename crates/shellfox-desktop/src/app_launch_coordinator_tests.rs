use super::*;
use shellfox_service::desktop_launch::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};
#[path = "app_launch_cancellation_tests.rs"]
mod cancellation;

#[derive(Default)]
struct Sessions {
    lost: bool,
    creations: usize,
    requests: BTreeMap<SessionId, SessionId>,
    sessions: BTreeSet<SessionId>,
}
impl LaunchAuthority for Sessions {
    fn check_owned(&self) -> Result<(), LaunchError> {
        if self.lost {
            Err(LaunchError::StaleOwner)
        } else {
            Ok(())
        }
    }
}
impl LaunchSessionResolver for Sessions {
    fn resolve(
        &mut self,
        intent: &DesktopLaunchIntent,
        kind: LaunchRequestKind,
    ) -> Result<LaunchTarget, LaunchError> {
        match intent.action {
            LaunchAction::Show {} => Ok(LaunchTarget::Show {}),
            LaunchAction::OpenSession { session } => {
                if self.sessions.contains(&session) {
                    Ok(LaunchTarget::Session { session, tab: None })
                } else {
                    Err(LaunchError::NotFound)
                }
            }
            LaunchAction::NewSession { .. } => {
                let session = match self.requests.get(&intent.request_id) {
                    Some(id) => *id,
                    None if kind == LaunchRequestKind::Known => {
                        return Err(LaunchError::ReceiptUnavailable);
                    }
                    None => {
                        let id = SessionId::new();
                        self.creations += 1;
                        self.requests.insert(intent.request_id, id);
                        self.sessions.insert(id);
                        id
                    }
                };
                if !self.sessions.contains(&session) {
                    return Err(LaunchError::ReceiptUnavailable);
                }
                Ok(LaunchTarget::Session { session, tab: None })
            }
        }
    }
}
struct Broker {
    router: DesktopLaunchRouter,
    host: Sessions,
}
struct DeliveryClient {
    broker: Rc<RefCell<Broker>>,
    claims: Cell<usize>,
    acknowledgements: Cell<usize>,
    wait_ack: Cell<bool>,
    invalid_ack: Cell<bool>,
}
impl DeliveryClient {
    fn new() -> (Self, AppLaunchCoordinator) {
        let host = Sessions::default();
        let mut router = DesktopLaunchRouter::new();
        let registration = router.register(router.owner(), &host).unwrap();
        (
            Self {
                broker: Rc::new(RefCell::new(Broker { router, host })),
                claims: Cell::new(0),
                acknowledgements: Cell::new(0),
                wait_ack: Cell::new(false),
                invalid_ack: Cell::new(false),
            },
            AppLaunchCoordinator::new(registration),
        )
    }
    fn queue(&self, request: DesktopLaunchIntent) {
        let mut broker = self.broker.borrow_mut();
        let Broker { router, host } = &mut *broker;
        router.enqueue(router.owner(), request, host).unwrap();
        router.dispatch_one(router.owner(), host).unwrap();
    }
}
impl LaunchDeliveryClient for DeliveryClient {
    fn claim<'a>(&'a self, delivery: &'a DesktopLaunchDelivery) -> LaunchFuture<'a, LaunchClaim> {
        Box::pin(async move {
            self.claims.set(self.claims.get() + 1);
            let mut broker = self.broker.borrow_mut();
            let Broker { router, host } = &mut *broker;
            router.claim(delivery, host)
        })
    }
    fn complete<'a>(
        &'a self,
        delivery: &'a DesktopLaunchDelivery,
        handling: LaunchHandling,
    ) -> LaunchFuture<'a, LaunchReceipt> {
        Box::pin(async move {
            self.acknowledgements.set(self.acknowledgements.get() + 1);
            if self.wait_ack.get() {
                std::future::pending::<()>().await;
            }
            let mut broker = self.broker.borrow_mut();
            let Broker { router, host } = &mut *broker;
            let mut receipt = router.complete(delivery, handling, host)?;
            if self.invalid_ack.get() {
                receipt.request_id = SessionId::new();
            }
            Ok(receipt)
        })
    }
}
#[derive(Default)]
struct Windows {
    calls: Vec<(SessionId, LaunchTarget, LaunchDisposition)>,
    local_selection: Vec<Option<SessionId>>,
    wait: bool,
}
impl LaunchWindowTransport for Windows {
    fn apply<'a>(
        &'a mut self,
        request_id: SessionId,
        target: &'a LaunchTarget,
        disposition: LaunchDisposition,
    ) -> LaunchFuture<'a, ()> {
        Box::pin(async move {
            self.calls.push((request_id, target.clone(), disposition));
            if let LaunchTarget::Session { session, .. } = target {
                if disposition == LaunchDisposition::NewWindow || self.local_selection.is_empty() {
                    self.local_selection.push(Some(*session));
                } else {
                    self.local_selection[0] = Some(*session);
                }
            }
            if self.wait {
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    }
}
fn poll_pending(future: std::pin::Pin<&mut impl Future>) {
    assert!(matches!(
        future.poll(&mut TaskContext::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_external_launch_updates_one_local_window_and_creates_no_second_session() {
    let (client, mut coordinator) = DeliveryClient::new();
    let untouched = SessionId::new();
    let mut windows = Windows {
        local_selection: vec![None, Some(untouched)],
        ..Default::default()
    };
    let request = request();
    client.queue(request.clone());
    let receipt = coordinator
        .pump(&client, &mut windows)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state, LaunchState::Handled);
    assert_eq!(windows.calls.len(), 1);
    assert_eq!(windows.local_selection[1], Some(untouched));
    assert_eq!(client.broker.borrow().host.creations, 1);
    assert_eq!(windows.calls[0].0, request.request_id);
    assert_eq!(coordinator.pump(&client, &mut windows).await.unwrap(), None);
    client.queue(request); // Completed request ID is not dispatched again.
    assert_eq!(coordinator.pump(&client, &mut windows).await.unwrap(), None);
    assert_eq!(windows.calls.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_open_session_new_window_has_session_only_target_and_keeps_other_windows() {
    let (client, mut coordinator) = DeliveryClient::new();
    let session = SessionId::new();
    client.broker.borrow_mut().host.sessions.insert(session);
    let original = SessionId::new();
    let mut windows = Windows {
        local_selection: vec![Some(original)],
        ..Default::default()
    };
    client.queue(DesktopLaunchIntent::new(
        LaunchAction::OpenSession { session },
        LaunchDisposition::NewWindow,
    ));
    coordinator.pump(&client, &mut windows).await.unwrap();
    assert_eq!(windows.local_selection, [Some(original), Some(session)]);
    assert_eq!(
        windows.calls[0].1,
        LaunchTarget::Session { session, tab: None }
    );
    assert_eq!(client.broker.borrow().host.creations, 0);
}
