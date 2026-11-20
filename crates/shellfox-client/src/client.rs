use shellfox_protocol::*;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::state::*;

pub(crate) fn is_connection_denial(method: &str, code: &ErrorCode) -> bool {
    match code {
        ErrorCode::AuthFailed | ErrorCode::ApprovalRequired => true,
        // A filesystem ACL or view-only denial rejects that operation, not the authenticated host view.
        ErrorCode::AccessDenied => !matches!(
            method,
            "listFiles"
                | "readFile"
                | "writeFile"
                | "writeImageFile"
                | "readFileAsset"
                | "completeDirectory"
                | "resolveDirectory"
                | "createDirectory"
                | "beginFileRead"
                | "readFileChunk"
                | "beginFileWrite"
                | "writeFileChunk"
                | "finishFileWrite"
                | "cancelFileTransfer"
        ),
        _ => false,
    }
}

/// One approved host view. Runtime drives pairing and polling; this crate spawns no tasks.
/// Mutations always make exactly one send. A transport error means outcome unknown.
pub struct Client<T> {
    transport: T,
    peer: AuthenticatedPeerId,
    session: Mutex<Session>,
}
impl<T: OutboundTransport> Client<T> {
    pub fn new(peer: AuthenticatedPeerId, client_id: ClientId, transport: T) -> Self {
        Self {
            transport,
            peer,
            session: Mutex::new(Session {
                state: ClientState {
                    status: ConnectionStatus::Disconnected,
                    permission: Permission::Unknown,
                    connection_id: 0,
                    last_error: None,
                    retry_after: None,
                },
                client_id,
                cycle: 0,
                cancel: CancellationToken::new(),
                failures: 0,
                pending: 0,
                polling: false,
                pairing: false,
            }),
        }
    }
    pub fn state(&self) -> ClientState {
        self.session.lock().expect("client state").state.clone()
    }
    pub fn peer(&self) -> &AuthenticatedPeerId {
        &self.peer
    }
    pub fn recovery_format(&self) -> Option<RecoveryFormat> {
        (self.state().status == ConnectionStatus::Connected).then_some(RecoveryFormat::AnsiV1)
    }
    /// Reconnect fences in-flight responses. Caller supplies a fresh cryptographic UUID.
    /// It does not resend any mutation or transfer terminal ownership.
    pub fn reconnect(&self, client_id: ClientId) {
        let mut s = self.session.lock().expect("client state");
        s.cancel.cancel();
        s.cancel = CancellationToken::new();
        s.cycle += 1;
        s.client_id = client_id;
        s.pending = 0;
        s.polling = false;
        s.pairing = false;
        s.state.status = ConnectionStatus::Reconnecting;
        s.state.permission = Permission::Unknown;
        s.state.retry_after = None;
        s.state.last_error = None;
    }
    /// Stops traffic immediately. Use disconnect() when the transport is still available.
    pub fn suspend(&self) {
        let mut s = self.session.lock().expect("client state");
        s.cancel.cancel();
        s.cycle += 1;
        s.pending = 0;
        s.polling = false;
        s.pairing = false;
        s.state.status = ConnectionStatus::Suspended;
        s.state.permission = Permission::Unknown;
    }
    pub async fn pair(&self) -> Result<Reply<PairResponse>, ClientError> {
        let (id, cycle, cancel) = {
            let mut s = self.session.lock().expect("client state");
            if s.pairing || s.polling || s.pending > 0 {
                return Err(ClientError::TooManyRequests);
            }
            if s.cancel.is_cancelled() {
                return Err(ClientError::NotConnected);
            }
            s.pairing = true;
            s.state.status = ConnectionStatus::Connecting;
            (s.client_id, s.cycle, s.cancel.child_token())
        };
        let _guard = CallGuard {
            session: &self.session,
            cycle,
            kind: CallKind::Pair,
            cancel: cancel.clone(),
        };
        let result = self
            .send(Endpoint::Pair, &ViewIdentity { client_id: id }, cancel)
            .await;
        let mut s = self.session.lock().expect("client state");
        if s.cycle != cycle {
            return Err(ClientError::StaleConnection);
        }
        match &result {
            Ok(Reply::Success(_)) => {
                s.failures = 0;
                s.state.connection_id += 1;
                s.state.status = ConnectionStatus::Connected;
                s.state.permission = Permission::Approved;
                s.state.retry_after = None;
                s.state.last_error = None;
            }
            Ok(Reply::Failure(error)) => Self::failed(&mut s, Some(error)),
            Err(ClientError::Validation(_)) => Self::failed(
                &mut s,
                Some(&AppError {
                    code: ErrorCode::Unsupported,
                    message: "Host does not support the exclusive-v1 terminal contract.".into(),
                    retryable: false,
                }),
            ),
            Err(_) => Self::failed(&mut s, None),
        }
        result
    }
    pub async fn execute<R: Request>(
        &self,
        request: &R,
    ) -> Result<Reply<R::Response>, ClientError> {
        self.execute_in_connection(request, None).await
    }

    /// Continuation of a multi-request effect. Never dispatch on a newly paired connection.
    pub async fn execute_with_connection<R: Request>(
        &self,
        connection: u64,
        request: &R,
    ) -> Result<Reply<R::Response>, ClientError> {
        self.execute_in_connection(request, Some(connection)).await
    }

    pub fn with_connection<R>(
        &self,
        connection: u64,
        operation: impl FnOnce() -> R,
    ) -> Result<R, ClientError> {
        let session = self.session.lock().expect("client state");
        if session.state.status != ConnectionStatus::Connected
            || session.state.connection_id != connection
        {
            return Err(ClientError::StaleConnection);
        }
        Ok(operation())
    }

    async fn execute_in_connection<R: Request>(
        &self,
        request: &R,
        expected: Option<u64>,
    ) -> Result<Reply<R::Response>, ClientError> {
        request.validate_wire()?;
        let (id, cycle, cancel) = {
            let mut s = self.session.lock().expect("client state");
            if s.state.status != ConnectionStatus::Connected {
                return Err(ClientError::NotConnected);
            }
            if expected.is_some_and(|id| s.state.connection_id != id) {
                return Err(ClientError::StaleConnection);
            }
            if s.pending >= 8 {
                return Err(ClientError::TooManyRequests);
            }
            s.pending += 1;
            (s.client_id, s.cycle, s.cancel.child_token())
        };
        let _guard = CallGuard {
            session: &self.session,
            cycle,
            kind: CallKind::Request,
            cancel: cancel.clone(),
        };
        let mut result = self
            .send(
                Endpoint::Request,
                &RequestEnvelope::new(id, request),
                cancel,
            )
            .await;
        if R::LEGACY_UPLOAD {
            if let Ok(Reply::Failure(error)) = &mut result {
                if error.code == ErrorCode::AuthFailed
                    && error.message == "This operation is not available remotely."
                {
                    *error = AppError {
                        code: ErrorCode::Unsupported,
                        message: "Upgrade the host to attach files to a terminal.".into(),
                        retryable: false,
                    };
                }
            }
        }
        let mut s = self.session.lock().expect("client state");
        if s.cycle != cycle {
            return Err(ClientError::StaleConnection);
        }
        match &result {
            Ok(Reply::Failure(error)) if is_connection_denial(R::METHOD, &error.code) => {
                Self::failed(&mut s, Some(error))
            }
            Err(ClientError::Transport(TransportError::Cancelled)) => {}
            Err(_) => Self::failed(&mut s, None),
            _ => {}
        }
        result
    }
    /// Returns validated events without ACKing. The view ACKs only after consuming data.
    pub async fn poll(&self) -> Result<Reply<PollResponse>, ClientError> {
        let (id, cycle, cancel) = {
            let mut s = self.session.lock().expect("client state");
            if s.state.status != ConnectionStatus::Connected {
                return Err(ClientError::NotConnected);
            }
            if s.polling {
                return Err(ClientError::TooManyRequests);
            }
            s.polling = true;
            (s.client_id, s.cycle, s.cancel.child_token())
        };
        let _guard = CallGuard {
            session: &self.session,
            cycle,
            kind: CallKind::Poll,
            cancel: cancel.clone(),
        };
        let result = self
            .send(Endpoint::Events, &ViewIdentity { client_id: id }, cancel)
            .await;
        let mut s = self.session.lock().expect("client state");
        if s.cycle != cycle {
            return Err(ClientError::StaleConnection);
        }
        match &result {
            Ok(Reply::Failure(error)) => Self::failed(&mut s, Some(error)),
            Err(ClientError::Transport(TransportError::Cancelled)) => {}
            Err(_) => Self::failed(&mut s, None),
            _ => {}
        }
        result
    }
    /// Withdraws this view only. Never closes tabs, shells or the host.
    pub async fn disconnect(&self) -> Result<Reply<Disconnected>, ClientError> {
        let id = {
            let mut s = self.session.lock().expect("client state");
            s.cancel.cancel();
            s.cycle += 1;
            s.pending = 0;
            s.polling = false;
            s.pairing = false;
            s.state.status = ConnectionStatus::Disconnected;
            s.state.permission = Permission::Unknown;
            s.state.retry_after = None;
            s.client_id
        };
        self.send(
            Endpoint::Disconnect,
            &ViewIdentity { client_id: id },
            CancellationToken::new(),
        )
        .await
    }
    fn failed(s: &mut Session, error: Option<&AppError>) {
        s.cancel.cancel();
        s.cancel = CancellationToken::new();
        s.cycle += 1;
        s.pending = 0;
        s.polling = false;
        s.pairing = false;
        s.state.permission = error
            .map(Permission::from_error)
            .unwrap_or(Permission::Unknown);
        s.state.last_error = error.cloned();
        let unsupported = error.is_some_and(|e| e.code == ErrorCode::Unsupported && !e.retryable);
        s.state.status = if unsupported {
            ConnectionStatus::Rejected
        } else {
            match s.state.permission {
                Permission::Rejected => ConnectionStatus::Rejected,
                Permission::AwaitingApproval => ConnectionStatus::AwaitingApproval,
                _ => ConnectionStatus::Reconnecting,
            }
        };
        s.state.retry_after = if unsupported || s.state.permission == Permission::Rejected {
            None
        } else if s.state.permission == Permission::AwaitingApproval {
            Some(Duration::from_secs(1))
        } else {
            let secs = (1_u64 << s.failures.min(5)).min(30);
            s.failures = s.failures.saturating_add(1);
            Some(Duration::from_secs(secs))
        };
    }
    async fn send<B: serde::Serialize, R: Validate + serde::de::DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        body: &B,
        cancel: CancellationToken,
    ) -> Result<Reply<R>, ClientError> {
        let bytes = serde_json::to_vec(body).map_err(|_| ValidationError)?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(ClientError::Validation(ValidationError));
        }
        if cancel.is_cancelled() {
            return Err(ClientError::Transport(TransportError::Cancelled));
        }
        let limits = RequestLimits {
            deadline: Instant::now() + Duration::from_secs(30),
            cancel: cancel.clone(),
            max_request_bytes: MAX_REQUEST_BYTES,
            max_response_bytes: MAX_RESPONSE_BYTES,
        };
        let response = self
            .transport
            .send(&self.peer, endpoint, bytes, limits.clone())
            .await
            .map_err(ClientError::Transport)?;
        if cancel.is_cancelled() {
            return Err(ClientError::Transport(TransportError::Cancelled));
        }
        if Instant::now() > limits.deadline {
            return Err(ClientError::Transport(TransportError::Deadline));
        }
        Ok(decode::<Reply<R>>(&response)?)
    }
}
