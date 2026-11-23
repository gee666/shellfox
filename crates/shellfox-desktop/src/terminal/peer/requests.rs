use super::V1Binding;
use shellfox_protocol::{
    AcknowledgeTerminal, AttachTerminal, DetachTerminal, ErrorCode, Generation, Reply,
    ResizeTerminal, TabId, TakeTerminalControl, TerminalAttachment, WriteTerminal,
};

pub(super) enum Operation {
    Attach {
        tab_id: TabId,
        takeover: Option<Generation>,
    },
    Ack(AcknowledgeTerminal),
    Detach(DetachTerminal),
    Resize(ResizeTerminal),
    Write(WriteTerminal),
}

pub(super) enum Applied {
    Attached(TerminalAttachment),
    Detached,
    Other,
}

impl Operation {
    pub async fn run(self, binding: &V1Binding) -> anyhow::Result<Reply<Applied>> {
        fn applied<T>(result: Reply<T>) -> Reply<Applied> {
            match result {
                Reply::Success(_) => Reply::Success(Applied::Other),
                Reply::Failure(error) => Reply::Failure(error),
            }
        }
        match self {
            Self::Attach { tab_id, takeover } => {
                if let Some(generation) = takeover
                    && let Reply::Failure(error) = binding
                        .v1_reply(&TakeTerminalControl { tab_id, generation })
                        .await?
                {
                    return Ok(Reply::Failure(error));
                }
                Ok(
                    match binding
                        .v1_reply(&AttachTerminal {
                            tab_id,
                            generation: takeover,
                            after_sequence: None,
                        })
                        .await?
                    {
                        Reply::Success(value) => Reply::Success(Applied::Attached(value)),
                        Reply::Failure(error) => Reply::Failure(error),
                    },
                )
            }
            Self::Detach(request) => Ok(match binding.v1_reply(&request).await? {
                Reply::Success(_) => Reply::Success(Applied::Detached),
                Reply::Failure(error)
                    if matches!(error.code, ErrorCode::NotFound | ErrorCode::TargetLost) =>
                {
                    Reply::Success(Applied::Detached)
                }
                Reply::Failure(error) => Reply::Failure(error),
            }),
            Self::Ack(request) => Ok(applied(binding.v1_reply(&request).await?)),
            Self::Resize(request) => Ok(applied(binding.v1_reply(&request).await?)),
            Self::Write(request) => Ok(applied(binding.v1_reply(&request).await?)),
        }
    }
}
