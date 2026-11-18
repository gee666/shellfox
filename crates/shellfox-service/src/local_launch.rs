use super::*;
use crate::desktop_launch::*;
use std::future::Future;

impl LocalClient {
    /// Ordinary launch-only handoff discovers the current owner once.
    pub async fn launch_existing(
        &self,
        intent: DesktopLaunchIntent,
        submission: LaunchRequestKind,
    ) -> Result<LaunchReceipt> {
        launch_existing_request(intent, submission, None, |request| async move {
            self.request(&request).await
        })
        .await
    }
    /// Internal fenced handoff never adopts a replacement owner. This never
    /// constructs a rendering client, registers a desktop, or starts a Host.
    pub async fn launch_existing_pinned(
        &self,
        intent: DesktopLaunchIntent,
        submission: LaunchRequestKind,
        expected_owner: LaunchOwnerFence,
    ) -> Result<LaunchReceipt> {
        launch_existing_request(
            intent,
            submission,
            Some(expected_owner),
            |request| async move { self.request(&request).await },
        )
        .await
    }
}

async fn launch_existing_request<F, R>(
    intent: DesktopLaunchIntent,
    submission: LaunchRequestKind,
    expected_owner: Option<LaunchOwnerFence>,
    mut request: F,
) -> Result<LaunchReceipt>
where
    F: FnMut(DesktopLaunchRequest) -> R,
    R: Future<Output = Result<DesktopLaunchReply>>,
{
    intent.validate()?;
    let DesktopLaunchReply::Route(route) = request(DesktopLaunchRequest::Discover {}).await? else {
        bail!("Unexpected desktop discovery response.");
    };
    if expected_owner.is_some_and(|expected| expected != route.owner) {
        return Err(LaunchError::StaleOwner.into());
    }
    if !route.desktop_registered {
        bail!(
            "Owner exists but no desktop is registered. No launch intent was submitted; launch-only handoff cannot render or start another Host."
        );
    }
    let request_id = intent.request_id;
    let mut reply = request(DesktopLaunchRequest::Submit { owner: route.owner, intent, submission }).await
        .with_context(|| format!("Desktop submission outcome may be uncertain; it was not replayed. Request ID: {request_id}"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let DesktopLaunchReply::Receipt(receipt) = reply else {
            bail!("Unexpected desktop launch response; submission was not replayed.");
        };
        if receipt.request_id != request_id {
            return Err(LaunchError::Invalid.into());
        }
        match receipt.state {
            LaunchState::Handled => return Ok(receipt),
            LaunchState::Failed(error) => return Err(error.into()),
            LaunchState::Uncertain => bail!(
                "Desktop launch outcome is uncertain; submission was not replayed. Request ID: {request_id}"
            ),
            LaunchState::Queued | LaunchState::Dispatched | LaunchState::Applying => {}
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "Desktop launch receipt is still pending; submission was not replayed. Request ID: {request_id}"
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        // Receipt reads retain the exact same owner. They never rediscover or resubmit.
        reply = request(DesktopLaunchRequest::Receipt {
            owner: route.owner,
            request_id,
        })
        .await
        .with_context(|| {
            format!(
                "Desktop receipt unavailable; submission was not replayed. Request ID: {request_id}"
            )
        })?;
    }
}

#[cfg(test)]
#[path = "local_launch_tests.rs"]
mod tests;
