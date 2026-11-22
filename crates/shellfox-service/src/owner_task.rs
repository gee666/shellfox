use super::{private, ExperimentRequest, ExperimentResponse};
use anyhow::Result;
use tokio::sync::oneshot;

pub(super) enum Task {
    Execute {
        cancel: shellfox_protocol::CancellationToken,
        request: ExperimentRequest,
        client_id: uuid::Uuid,
        reply: oneshot::Sender<Result<ExperimentResponse>>,
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
        reply: oneshot::Sender<crate::activity_integration::SourceFactsWatch>,
    },
    /// Work arrived outside this inbox; run one owner pass.
    Wake,
    Shutdown,
}
