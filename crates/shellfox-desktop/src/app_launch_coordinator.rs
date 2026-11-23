use super::{LaunchDisposition, LaunchError};
use shellfox_core::SessionId;
use shellfox_service::desktop_launch::{
    DesktopClientToken, DesktopLaunchDelivery, DesktopRegistration, LaunchClaim, LaunchHandling,
    LaunchReceipt, LaunchState, LaunchTarget,
};
use std::{future::Future, pin::Pin};
use tokio::sync::mpsc;

pub type LaunchFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, LaunchError>> + 'a>>;

/// Native trusted path only, never the public/mobile request transport.
pub trait LaunchDeliveryClient {
    fn claim<'a>(&'a self, delivery: &'a DesktopLaunchDelivery) -> LaunchFuture<'a, LaunchClaim>;
    fn complete<'a>(
        &'a self,
        delivery: &'a DesktopLaunchDelivery,
        handling: LaunchHandling,
    ) -> LaunchFuture<'a, LaunchReceipt>;
}
pub trait LaunchWindowTransport {
    /// Apply to ONE chosen window or open ONE new window with local selection.
    /// No global selection broadcast, PTY creation, implicit takeover or observer.
    fn apply<'a>(
        &'a mut self,
        request_id: SessionId,
        target: &'a LaunchTarget,
        disposition: LaunchDisposition,
    ) -> LaunchFuture<'a, ()>;
}
#[derive(Clone, Copy)]
enum Phase {
    Received,
    Applying,
    Applied(LaunchHandling),
}
struct Pending {
    delivery: DesktopLaunchDelivery,
    phase: Phase,
}
/// One non-cloneable coordinator per app, consuming its unique registration.
/// It remains app-owned when an individual window or its poll task disappears.
pub struct AppLaunchCoordinator {
    token: DesktopClientToken,
    receiver: mpsc::Receiver<DesktopLaunchDelivery>,
    /// Received by `ready`, not yet taken by `pump`.
    next: Option<DesktopLaunchDelivery>,
    pending: Option<Pending>,
}
impl AppLaunchCoordinator {
    pub fn new(registration: DesktopRegistration) -> Self {
        Self {
            token: registration.token,
            receiver: registration.receiver,
            next: None,
            pending: None,
        }
    }
    /// Waits until a delivery arrived or work is pending. StaleClient once the
    /// host side of the registration is gone.
    pub async fn ready(&mut self) -> Result<(), LaunchError> {
        if self.pending.is_none() && self.next.is_none() {
            self.next = Some(self.receiver.recv().await.ok_or(LaunchError::StaleClient)?);
        }
        Ok(())
    }
    /// At most one queued launch per pump. A cancelled ACK wait resumes only the
    /// ACK. A cancelled native apply is uncertain and is NEVER invoked twice.
    pub async fn pump(
        &mut self,
        client: &impl LaunchDeliveryClient,
        windows: &mut impl LaunchWindowTransport,
    ) -> Result<Option<LaunchReceipt>, LaunchError> {
        if self.pending.is_none() {
            let delivery = match self.next.take().map_or_else(|| self.receiver.try_recv(), Ok) {
                Ok(delivery) => delivery,
                Err(mpsc::error::TryRecvError::Empty) => return Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err(LaunchError::StaleClient);
                }
            };
            delivery.validate()?;
            if delivery.client != self.token {
                return Err(LaunchError::StaleClient);
            }
            self.pending = Some(Pending {
                delivery,
                phase: Phase::Received,
            });
        }
        let pending = self.pending.as_mut().ok_or(LaunchError::Unavailable)?;
        match pending.phase {
            Phase::Received => {
                let claim = match client.claim(&pending.delivery).await {
                    Ok(claim) => claim,
                    Err(error) => {
                        if !matches!(error, LaunchError::Unavailable | LaunchError::Busy) {
                            self.pending = None;
                        }
                        return Err(error);
                    }
                };
                match claim {
                    LaunchClaim::AlreadyHandled => {
                        let receipt = LaunchReceipt {
                            request_id: pending.delivery.intent.request_id,
                            state: LaunchState::Handled,
                        };
                        self.pending = None;
                        return Ok(Some(receipt));
                    }
                    LaunchClaim::Apply => {
                        pending.phase = Phase::Applying;
                        let result = windows
                            .apply(
                                pending.delivery.intent.request_id,
                                &pending.delivery.target,
                                pending.delivery.intent.disposition,
                            )
                            .await;
                        pending.phase = Phase::Applied(match result {
                            Ok(()) => LaunchHandling::Handled,
                            Err(error) => LaunchHandling::Failed(error),
                        });
                    }
                }
            }
            Phase::Applying => {
                pending.phase =
                    Phase::Applied(LaunchHandling::Failed(LaunchError::DeliveryUncertain))
            }
            Phase::Applied(_) => {}
        }
        let Phase::Applied(handling) = pending.phase else {
            return Err(LaunchError::DeliveryUncertain);
        };
        let receipt = client.complete(&pending.delivery, handling).await?;
        let expected = match handling {
            LaunchHandling::Handled => LaunchState::Handled,
            LaunchHandling::Failed(LaunchError::DeliveryUncertain) => LaunchState::Uncertain,
            LaunchHandling::Failed(error) => LaunchState::Failed(error),
        };
        if receipt.request_id != pending.delivery.intent.request_id || receipt.state != expected {
            return Err(LaunchError::Invalid);
        }
        self.pending = None;
        Ok(Some(receipt))
    }
}
