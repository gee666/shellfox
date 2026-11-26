use super::{private, LocalApiRequest, LocalApiResponse};
use anyhow::Result;
use tokio::sync::oneshot;

pub(super) enum Task {
    Execute {
        cancel: shellfox_protocol::CancellationToken,
        request: LocalApiRequest,
        client_id: uuid::Uuid,
        cause: super::Cause,
        reply: oneshot::Sender<Result<LocalApiResponse>>,
    },
    Private(Box<private::PrivateTask>, super::Cause),
    Native {
        cancel: shellfox_protocol::CancellationToken,
        action: Box<crate::native::NativeAction>,
        client_id: uuid::Uuid,
        cause: super::Cause,
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
    /// The request this task answers; its reply waits for the changes it made.
    pub(super) fn cause(&self) -> Option<super::Cause> {
        match self {
            Self::Execute { cause, .. } | Self::Native { cause, .. } | Self::Private(_, cause) => {
                Some(*cause)
            }
            Self::Approvals(task) => Some(task.cause()),
            _ => None,
        }
    }
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
            Self::Private(task, _) => task.flows(service),
            // Shutdown only wakes the loop; `abandon` ends it, and shutdown fails the close.
            Self::Shutdown | Self::InstallRemote(_) => false,
        }
    }
}

impl Task {
    /// The tab an attach would start, for a valid request. The owner starts it
    /// first and runs the request once the start settled (see owner.rs).
    pub(super) fn start_target(
        &self,
        private: &super::private::PrivateOwner,
    ) -> Option<shellfox_core::TabId> {
        use shellfox_protocol::Validate;
        match self {
            Self::Execute {
                request: request @ LocalApiRequest::Attach { tab, .. },
                ..
            } => request.validate().is_ok().then_some(*tab),
            Self::Native { action, .. } => match action.as_ref() {
                crate::native::NativeAction::Attach { tab, .. } => {
                    action.validate().is_ok().then_some(*tab)
                }
                _ => None,
            },
            Self::Private(task, _) => task.start_target(private),
            _ => None,
        }
    }
    /// The client a request comes from.
    pub(super) fn client(&self) -> Option<uuid::Uuid> {
        match self {
            Self::Execute { client_id, .. } | Self::Native { client_id, .. } => Some(*client_id),
            Self::Private(task, _) => task.client(),
            _ => None,
        }
    }
    /// The client whose controller this request detaches.
    pub(super) fn detaches(&self) -> Option<uuid::Uuid> {
        let detaching = match self {
            Self::Execute { request, .. } => matches!(request, LocalApiRequest::Detach {}),
            Self::Native { action, .. } => {
                matches!(action.as_ref(), crate::native::NativeAction::DetachAll)
            }
            Self::Private(task, _) => task.detaches(),
            _ => false,
        };
        detaching.then(|| self.client()).flatten()
    }
    /// Answers a request with `error` without running it.
    pub(super) fn fail(self, error: anyhow::Error) {
        match self {
            Self::Execute { reply, .. } => drop(reply.send(Err(error))),
            Self::Native { reply, .. } => drop(reply.send(Err(error))),
            Self::Private(task, _) => task.fail(error),
            _ => {}
        }
    }
}
