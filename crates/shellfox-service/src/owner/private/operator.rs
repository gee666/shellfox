use super::*;
use crate::remote::{
    native::{RemoteAccepted, RemoteAdmissionError},
    operator::{self, Reply},
    RemoteControl, RemoteRequest,
};

fn admission_error(message: &str) -> shellfox_protocol::AppError {
    shellfox_protocol::AppError {
        code: shellfox_protocol::ErrorCode::NativeUnavailable,
        message: message.into(),
        retryable: false,
    }
}
pub(in crate::owner) struct Submission {
    client_id: uuid::Uuid,
    epoch: uuid::Uuid,
    control: RemoteControl,
    request: RemoteRequest,
    reply:
        oneshot::Sender<Result<std::result::Result<RemoteAccepted, shellfox_protocol::AppError>>>,
}
impl PrivateOwner {
    pub(super) fn submit_remote_operator(&self, service: &Service, task: Submission) {
        if task.reply.is_closed() {
            return;
        }
        let result = self
            .caller_fence(service, task.client_id, task.epoch)
            .and_then(|fence| {
                anyhow::ensure!(
                    !fence.is_cancelled(),
                    "Original local operator lifetime closed before admission"
                );
                // Epoch validation and the original Manager enqueue occur in this
                // SAME actor turn. Never await or reconstruct an owner from a label.
                Ok(task
                    .control
                    .try_submit(task.request)
                    .map_err(|error| match error {
                        RemoteAdmissionError::Invalid(error) => error,
                        RemoteAdmissionError::Full(_) => admission_error(
                            "Remote operator queue is full; action was not accepted.",
                        ),
                        RemoteAdmissionError::Stopped(_) => {
                            admission_error("Remote operator stopped; action was not accepted.")
                        }
                    }))
            });
        let _ = task.reply.send(result);
    }
}
impl OwnerClient {
    pub(crate) async fn private_remote_operator(
        &self,
        request: RemoteRequest,
        epoch: uuid::Uuid,
    ) -> Result<serde_json::Value> {
        let control = self
            .remote
            .as_ref()
            .context("Original Host has no remote operator installed")?
            .clone();
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::RemoteOperator(Box::new(Submission {
            client_id: self.client_id,
            epoch,
            control,
            request,
            reply,
        })))
        .await?;
        let admitted = response
            .await
            .context("Operator admission receipt lost; outcome unknown, not retried")??;
        let result: Reply = match admitted {
            Ok(receive) => receive
                .await
                .context("Accepted operator response lost; outcome unknown, not retried")?
                .map(|receipt| operator::RemoteOperatorState::from(&receipt.state)),
            Err(error) => Err(error),
        };
        if let Ok(state) = &result {
            operator::validate_state(state)
                .map_err(|_| anyhow::anyhow!("Invalid original remote operator state"))?;
        }
        // Accepted Manager lifecycle/approval work remains in that actor even
        // when this caller/response is dropped. Dropping is never a stop ACK.
        Ok(serde_json::to_value(result)?)
    }
}
