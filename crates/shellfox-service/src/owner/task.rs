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
