//! ControlRequired is a blocked terminal, not permission to observe or take it.
use crate::{app_host::HostFuture, host_client::HostClient};
use shellfox_core::TabId;
use shellfox_runtime::{ErrorCode, RuntimeError};
use shellfox_service::Attachment;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachIntent {
    Open,
    /// Only the user's explicit "Take it" action may request takeover.
    TakeIt,
}
pub enum AttachOutcome {
    Attached(Attachment),
    ControlRequired,
    Failed(anyhow::Error),
}
pub trait WindowAttach {
    fn attach(&self, tab: TabId, takeover: bool) -> HostFuture<'_, Attachment>;
}
impl WindowAttach for HostClient {
    fn attach(&self, tab: TabId, takeover: bool) -> HostFuture<'_, Attachment> {
        Box::pin(HostClient::attach(self, tab, takeover))
    }
}
pub async fn attach(client: &impl WindowAttach, tab: TabId, intent: AttachIntent) -> AttachOutcome {
    match client.attach(tab, intent == AttachIntent::TakeIt).await {
        Ok(attachment) => AttachOutcome::Attached(attachment),
        Err(error) if is_control_required(&error) => AttachOutcome::ControlRequired,
        Err(error) => AttachOutcome::Failed(error),
    }
}
pub fn is_control_required(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<RuntimeError>()
        .is_some_and(|error| error.code == ErrorCode::ControlRequired)
}
