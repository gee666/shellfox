use super::*;
mod outgoing;
mod remote_operator;

#[derive(Clone)]
struct LocalTransport {
    record: Record,
    directory: std::path::PathBuf,
    #[cfg(unix)]
    parent: std::sync::Arc<std::fs::File>,
    client_id: uuid::Uuid,
    epoch: uuid::Uuid,
}
impl LocalTransportContract for LocalTransport {
    fn send<'a>(&'a self, body: Vec<u8>, limits: RequestLimits) -> TransportFuture<'a> {
        Box::pin(async move {
            if body.len() > limits.max_request_bytes {
                return Err(TransportError::TooLarge);
            }
            let deadline = tokio::time::Instant::from_std(limits.deadline);
            let operation = async {
                let mut stream = platform::connect_verified(
                    &self.record.address,
                    &self.directory,
                    #[cfg(unix)]
                    &self.parent,
                )
                .await
                .map_err(|_| TransportError::Unavailable)?;
                let frame = serde_json::to_vec(&Frame::Request {
                    version: TRANSPORT_VERSION,
                    secret: self.record.secret.clone(),
                    client_id: self.client_id,
                    epoch: self.epoch,
                    body,
                })
                .map_err(|_| TransportError::TooLarge)?;
                write_frame(&mut stream, &frame)
                    .await
                    .map_err(|_| TransportError::Unavailable)?;
                let response = read_frame(&mut stream, limits.max_response_bytes)
                    .await
                    .map_err(|_| TransportError::Unavailable)?;
                let mut receipt = [0_u8; 16];
                stream
                    .read_exact(&mut receipt)
                    .await
                    .map_err(|_| TransportError::Unavailable)?;
                stream
                    .write_all(&receipt)
                    .await
                    .map_err(|_| TransportError::Unavailable)?;
                stream
                    .flush()
                    .await
                    .map_err(|_| TransportError::Unavailable)?;
                Ok(response)
            };
            tokio::pin!(operation);
            loop {
                if limits.cancel.is_cancelled() {
                    return Err(TransportError::Cancelled);
                }
                if Instant::now() >= limits.deadline {
                    return Err(TransportError::Deadline);
                }
                tokio::select! {
                    result = &mut operation => return result,
                    _ = tokio::time::sleep_until(deadline) => return Err(TransportError::Deadline),
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                }
            }
        })
    }
}

/// A validated private operation failure, distinct from transport uncertainty.
#[derive(Debug)]
pub struct LocalRequestFailure {
    pub method: &'static str,
    pub error: shellfox_protocol::AppError,
}
impl std::fmt::Display for LocalRequestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error.message)
    }
}
impl std::error::Error for LocalRequestFailure {}
impl LocalRequestFailure {
    pub fn automation_stdin_closed(&self) -> bool {
        use shellfox_protocol::LocalRequest;
        self.method == shellfox_protocol::automation::AutomationWrite::METHOD
            && self.error.code == shellfox_protocol::ErrorCode::TargetLost
    }
}
/// Uses the shared typed client. It never opens SQLite or launches a shell.
pub struct LocalClient {
    client: shellfox_client::LocalClient<LocalTransport>,
    transport: LocalTransport,
    // No reader task/transport clone owns this socket. Drop really causes EOF.
    lifetime: std::sync::Mutex<Option<platform::LocalStream>>,
}
impl LocalClient {
    pub async fn connect(data: &Path, helper: &Path, helper_sha256: [u8; 32]) -> Result<Self> {
        let (private, bytes) = PrivateDirectory::read_existing(data, helper, helper_sha256).await?;
        let record: Record = serde_json::from_slice(&bytes)?;
        if record.namespace != NAMESPACE
            || record.transport_version != TRANSPORT_VERSION
            || record.secret.len() != 64
            || !record.secret.bytes().all(|b| b.is_ascii_hexdigit())
        {
            bail!("Invalid experimental endpoint record.");
        }
        let client_id = uuid::Uuid::new_v4();
        #[cfg(unix)]
        let parent = std::sync::Arc::new(private.endpoint_directory()?);
        let (lifetime, epoch) = tokio::time::timeout(Duration::from_secs(30), async {
            let mut stream = platform::connect_verified(
                &record.address,
                &private.path,
                #[cfg(unix)]
                &parent,
            )
            .await?;
            write_frame(
                &mut stream,
                &serde_json::to_vec(&Frame::LifetimeOpen {
                    version: TRANSPORT_VERSION,
                    secret: record.secret.clone(),
                    client_id,
                })?,
            )
            .await?;
            let bytes = read_frame(&mut stream, 65536).await?;
            let LifetimeReply::Opened {
                version,
                client_id: opened,
                epoch,
            } = serde_json::from_slice(&bytes)?;
            if version != TRANSPORT_VERSION || opened != client_id || epoch.is_nil() {
                bail!("Invalid private lifetime opening.");
            }
            let mut receipt = [0; 16];
            stream.read_exact(&mut receipt).await?;
            stream.write_all(&receipt).await?;
            stream.flush().await?;
            Ok::<_, anyhow::Error>((stream, epoch))
        })
        .await
        .context("Private lifetime opening deadline.")??;
        let transport = LocalTransport {
            record,
            directory: private.path,
            #[cfg(unix)]
            parent,
            client_id,
            epoch,
        };
        let client = shellfox_client::LocalClient::new(client_id, transport.clone());
        Ok(Self {
            client,
            transport,
            lifetime: std::sync::Mutex::new(Some(lifetime)),
        })
    }
    /// Private, explicitly allowlisted local-host requests only. Admission and
    /// response validation stay in the shared client; this adds no method policy.
    pub async fn request<R: shellfox_protocol::LocalRequest>(
        &self,
        request: &R,
    ) -> Result<R::Response> {
        match self.client.execute(request).await? {
            shellfox_protocol::Reply::Success(value) => Ok(value),
            shellfox_protocol::Reply::Failure(error) => Err(LocalRequestFailure {
                method: R::METHOD,
                error,
            }
            .into()),
        }
    }
    pub async fn execute(&self, request: ExperimentRequest) -> Result<ExperimentResponse> {
        self.request(&request).await
    }
    pub async fn disconnect(&self) -> Result<()> {
        let result = self.execute(ExperimentRequest::Detach {}).await.map(|_| ());
        self.lifetime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        result
    }
    /// Resolves once the host closes this client's lifetime, at the latest when
    /// its process exits. Ends this client: call it after its last request.
    pub async fn closed(&self) -> HostClosed {
        let lifetime = self
            .lifetime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let mut closed = HostClosed::Lost;
        if let Some(mut lifetime) = lifetime {
            let mut byte = [0; 1];
            while matches!(lifetime.read(&mut byte).await, Ok(1..)) {
                if byte[0] == transport_frame::LIFETIME_STOPPING {
                    closed = HostClosed::Stopped;
                }
            }
        }
        closed
    }
}
/// How a host ended a client's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostClosed {
    /// An orderly stop: Stop request, signal or service manager.
    Stopped,
    /// The host died, or this client was already disconnected.
    Lost,
}
