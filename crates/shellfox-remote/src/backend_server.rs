use super::*;
use shellfox_protocol::Reply;
use shellfox_transport::LoopbackOrigin;
use tokio::{net::TcpListener, sync::Semaphore};

impl Backend {
    /// Binds only IPv4 loopback. Pass the returned origin and the same secret to
    /// the verified retained helper; neither value belongs in logs or argv.
    pub async fn bind(self: Arc<Self>) -> std::io::Result<(LoopbackOrigin, BackendHandle)> {
        self.bind_until(shellfox_protocol::CancellationToken::new())
            .await
    }
    /// Stop admission and close sockets when native host ownership is lost.
    pub async fn bind_until(
        self: Arc<Self>,
        lost: shellfox_protocol::CancellationToken,
    ) -> std::io::Result<(LoopbackOrigin, BackendHandle)> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let origin = LoopbackOrigin::new(address).map_err(std::io::Error::other)?;
        let task = tokio::spawn(async move {
            let permits = Arc::new(Semaphore::new(64));
            let mut tasks = tokio::task::JoinSet::new();
            let mut sweep = tokio::time::interval(Duration::from_secs(10));
            loop {
                if lost.is_cancelled() {
                    break;
                }
                // Finished tasks also occupy JoinSet entries. Reap them before
                // accepting indefinitely, even under a continuous local flood.
                while tasks.len() >= 128 {
                    tasks.join_next().await;
                }
                tokio::select! {
                    _ = lost.cancelled() => break,
                    connection = listener.accept() => {
                        let Ok((mut stream, address)) = connection else { break; };
                        if !address.ip().is_loopback() { continue; }
                        let Ok(permit) = permits.clone().try_acquire_owned() else { continue; };
                        let backend = self.clone();
                        tasks.spawn(async move {
                            let _permit = permit;
                            let operation = async {
                                let result = match tokio::time::timeout(Duration::from_secs(10), http::read(&mut stream)).await {
                                    Ok(Ok(request)) => {
                                        if let Some(endpoint)=request.manager_endpoint() {
                                            // The signed handler owns authentication, policy and
                                            // canonical V2 encoding. Never wrap it in legacy Reply.
                                            let reply=backend.handle_manager_v2_signed(endpoint,&request.body,request.signed_headers()).await;
                                            let _=http::reply_with_status(&mut stream,reply.status,&reply.body).await;
                                            return;
                                        }
                                        backend.handle(request).await
                                    },
                                    _ => Err(invalid()),
                                };
                                let reply = match result { Ok(value) => Reply::Success(value), Err(error) => Reply::Failure(error) };
                                if let Ok(bytes) = serde_json::to_vec(&reply) { let _ = http::reply(&mut stream, &bytes).await; }
                            };
                            let _ = tokio::time::timeout(Duration::from_secs(30), operation).await;
                        });
                    }
                    _ = sweep.tick() => {
                        tokio::select! { _ = lost.cancelled() => break, _ = self.sweep() => {} }
                    }
                    Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
                }
            }
        });
        Ok((origin, BackendHandle { task, address }))
    }
    async fn sweep(&self) {
        self.sweep_observer_v2().await;
        self.sweep_manager_v2().await;
        let now = (self.clock)();
        let _ = self.approvals.expire_pending_async(now).await;
        let views = if let Ok(limits) = self.limits.lock() {
            limits
                .views
                .iter()
                .map(|(id, view)| (id.clone(), view.epoch, view.touched, view.actor.clone()))
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        for (id, epoch, touched, actor) in views {
            if now.saturating_sub(touched) <= 60_000 && actor.validate_async().await.is_ok() {
                continue;
            }
            if self
                .host
                .request(OwnerCommand::Disconnect { actor })
                .await
                .is_ok()
            {
                let _ = self.remove_view(&id, epoch);
            }
        }
    }
    pub(super) fn remove_view(&self, id: &str, epoch: u64) -> Result<(), AppError> {
        let mut limits = self.limits.lock().map_err(|_| invalid())?;
        if limits.views.get(id).is_some_and(|view| view.epoch == epoch) {
            limits.views.remove(id);
        }
        Ok(())
    }
}
