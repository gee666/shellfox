use super::*;
use crate::native::{NativeAction, NativeAdmissionError, NativeResponse};

impl OwnerClient {
    /// Returns the actual owner's coalesced evidence channel, not a copied Host
    /// or Runtime. None remains explicit while evidence is pending/invalidated.
    pub async fn source_facts_watch(
        &self,
    ) -> Result<crate::activity::integration::SourceFactsWatch> {
        let (reply, receive) = oneshot::channel();
        self.sender
            .try_send(Task::SourceFactsWatch { reply })
            .map_err(|_| anyhow::anyhow!("Owner facts-watch request was not admitted."))?;
        receive
            .await
            .context("Owner stopped before returning its facts watch.")
    }
    pub async fn native_watches(&self) -> Result<crate::native::NativeWatches> {
        let (reply, receive) = oneshot::channel();
        self.sender
            .try_send(Task::Watches { reply })
            .map_err(|_| anyhow::anyhow!("Owner watch request was not admitted."))?;
        receive
            .await
            .context("Owner stopped before returning its watches.")
    }
    pub fn remote_state(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<crate::remote::native::RemoteControlState>> {
        self.remote
            .as_ref()
            .map(crate::remote::native::RemoteControl::subscribe)
    }
    pub fn try_native(
        &self,
        action: NativeAction,
    ) -> std::result::Result<NativeAccepted, NativeAdmissionError> {
        if self.client_id.is_nil() {
            return Err(NativeAdmissionError::Invalid(
                "A non-nil client ID is required.".into(),
            ));
        }
        action
            .validate()
            .map_err(|error| NativeAdmissionError::Invalid(error.to_string()))?;
        if let NativeAction::Remote(request) = action {
            use crate::remote::native::RemoteAdmissionError;
            let Some(control) = &self.remote else {
                // Stopped applies ONLY to this absent remote admission target.
                // The local actor remains usable; this never authorizes replay,
                // reacquisition or cold fallback.
                return Err(NativeAdmissionError::Stopped(Box::new(
                    NativeAction::Remote(request),
                )));
            };
            return control
                .try_submit(request)
                .map(NativeAccepted::Remote)
                .map_err(|error| match error {
                    RemoteAdmissionError::Full(request) => {
                        NativeAdmissionError::Full(Box::new(NativeAction::Remote(request)))
                    }
                    RemoteAdmissionError::Stopped(request) => {
                        NativeAdmissionError::Stopped(Box::new(NativeAction::Remote(request)))
                    }
                    RemoteAdmissionError::Invalid(error) => {
                        NativeAdmissionError::Invalid(error.message)
                    }
                });
        }
        let (reply, response) = oneshot::channel();
        let cancel = shellfox_protocol::CancellationToken::new();
        let cause = self.durability.cause();
        match self.sender.try_send(Task::Native {
            cancel: cancel.clone(),
            action: Box::new(action),
            client_id: self.client_id,
            cause,
            reply,
        }) {
            Ok(()) => Ok(NativeAccepted::owner(
                response,
                cancel,
                self.durability.settle(cause),
            )),
            Err(TrySendError::Full(Task::Native { action, .. })) => {
                Err(NativeAdmissionError::Full(action))
            }
            Err(TrySendError::Disconnected(Task::Native { action, .. })) => {
                Err(NativeAdmissionError::Stopped(action))
            }
            // Only a Native task is submitted by this method. Other variants
            // cannot cross admission and are never treated as retry signals.
            Err(_) => Err(NativeAdmissionError::Invalid(
                "Unexpected native admission task.".into(),
            )),
        }
    }
    pub async fn native(&self, action: NativeAction) -> Result<NativeResponse> {
        self.try_native(action)?.await
    }
    /// Bind only the remote manager created inside this host's owned startup.
    /// This does not create a second Host, runtime registry or controller ID.
    pub fn with_remote(mut self, remote: crate::remote::native::RemoteControl) -> Self {
        self.remote = Some(remote);
        self
    }
    pub fn fork(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            client_id: uuid::Uuid::new_v4(),
            remote: self.remote.clone(),
            durability: self.durability.clone(),
        }
    }
    #[cfg(test)]
    pub(crate) fn client_id(&self) -> uuid::Uuid {
        self.client_id
    }
    pub fn try_execute(&self, request: LocalApiRequest) -> Result<super::LocalApiAccepted> {
        use shellfox_protocol::Validate;
        request.validate()?;
        let (reply, response) = oneshot::channel();
        let recipient = crate::grant::requests::RecipientCancellation::new();
        let cause = self.durability.cause();
        match self.sender.try_send(Task::Execute {
            cancel: recipient.token(),
            request,
            client_id: self.client_id,
            cause,
            reply,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => bail!("Service owner queue is full."),
            Err(TrySendError::Disconnected(_)) => bail!("Service owner stopped."),
        }
        Ok(super::LocalApiAccepted::new(
            response,
            recipient,
            self.durability.settle(cause),
        ))
    }
    pub async fn execute(&self, request: LocalApiRequest) -> Result<LocalApiResponse> {
        self.try_execute(request)?
            .await
            .context("Service owner stopped before replying.")?
    }
}
