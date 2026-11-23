use super::{private, LocalApiRequest, LocalApiResponse};
use anyhow::Result;
use tokio::sync::oneshot;

pub(super) enum Task {
    Execute {
        cancel: shellfox_protocol::CancellationToken,
        request: LocalApiRequest,
        client_id: uuid::Uuid,
        reply: oneshot::Sender<Result<LocalApiResponse>>,
    },
    Private(Box<private::PrivateTask>),
    Native {
        cancel: shellfox_protocol::CancellationToken,
        action: Box<crate::native::NativeAction>,
        client_id: uuid::Uuid,
        reply: oneshot::Sender<Result<crate::native::NativeResponse>>,
    },
    Approvals(Box<super::approvals::ApprovalTask>),
    InstallRemote(std::sync::Arc<super::approvals::RemoteInstall>),
    Activate {
        reply: oneshot::Sender<()>,
    },
    SourceFactsWatch {
        reply: oneshot::Sender<crate::activity::integration::SourceFactsWatch>,
    },
    Watches {
        reply: oneshot::Sender<crate::native::NativeWatches>,
    },
    /// Work arrived outside this inbox; run one owner pass.
    Wake,
    Shutdown,
}

impl Task {
    /// Whether this task may run while a close is in flight (see close.rs).
    pub(super) fn flows(&self, service: &crate::Service) -> bool {
        match self {
            // Approvals have their own revision, so they don't invalidate a close.
            Self::Wake
            | Self::Approvals(_)
            | Self::Activate { .. }
            | Self::SourceFactsWatch { .. }
            | Self::Watches { .. } => true,
            Self::Execute { request, .. } => service.local_api_flows(request),
            Self::Native { action, .. } => service.native_flows(action),
            Self::Private(task) => task.flows(service),
            // Shutdown waits for the close, as it did when the owner blocked.
            Self::Shutdown | Self::InstallRemote(_) => false,
        }
    }
}
