#[cfg(test)]
#[path = "backend_http_tests.rs"]
mod http_tests;
#[path = "backend_manager.rs"]
mod manager;
#[path = "backend_observer.rs"]
mod observer;
#[path = "backend_server.rs"]
mod server;
pub use manager::ManagerHttpReply;
pub use observer::ObserverHttpReply;
#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
use crate::{error, http, Actor, ApprovalControl, OwnerCommand, RemoteHost};
use serde::Deserialize;
use serde_json::{json, Value};
use shellfox_protocol::{AppError, ClientId, ErrorCode, RemoteCommand, Validate};
use shellfox_transport::{incoming::PeerIdentity, Secret};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    #[serde(deserialize_with = "client_id")]
    client_id: ClientId,
}
fn client_id<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<ClientId, D::Error> {
    let raw = String::deserialize(deserializer)?;
    let identity: shellfox_protocol::ViewIdentity =
        serde_json::from_value(json!({"clientId":raw})).map_err(serde::de::Error::custom)?;
    Ok(identity.client_id)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    #[serde(deserialize_with = "client_id")]
    client_id: ClientId,
    method: String,
    payload: Value,
}
struct View {
    actor: Actor,
    touched: u64,
    epoch: u64,
    paired: bool,
    polling: bool,
    requests: u8,
}
#[derive(Default)]
struct Limits {
    last_clock: Option<u64>,
    replay: HashMap<String, u64>,
    rates: HashMap<String, (u64, u16)>,
    views: HashMap<String, View>,
    epoch: u64,
}
pub struct Backend {
    host: Arc<dyn RemoteHost>,
    approvals: ApprovalControl,
    secret: Secret,
    limits: Mutex<Limits>,
    clock: fn() -> u64,
    observer_v2: Option<observer::ObserverBackend>,
    manager_v2: Option<manager::ManagerBackend>,
}
/// Dropping the owner closes the listener and all accepted sockets.
pub struct BackendHandle {
    task: tokio::task::JoinHandle<()>,
    address: std::net::SocketAddr,
}
impl BackendHandle {
    pub fn address(&self) -> std::net::SocketAddr {
        self.address
    }
    pub async fn shutdown(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}
impl Drop for BackendHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub(crate) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn invalid() -> AppError {
    error(
        ErrorCode::Validation,
        "Invalid or bounded remote request.",
        false,
    )
}
fn auth() -> AppError {
    error(
        ErrorCode::AuthFailed,
        "Unverified remote peer or view. Pair again.",
        true,
    )
}
impl Backend {
    pub fn new(host: Arc<dyn RemoteHost>, approvals: ApprovalControl, secret: Secret) -> Self {
        Self {
            host,
            approvals,
            secret,
            limits: Mutex::new(Limits::default()),
            clock: now_millis,
            observer_v2: None,
            manager_v2: None,
        }
    }
    async fn handle(&self, request: http::Request) -> Result<Value, AppError> {
        self.handle_at(request, (self.clock)()).await
    }
    async fn handle_at(&self, request: http::Request, now: u64) -> Result<Value, AppError> {
        let peer = self
            .secret
            .verify_incoming(
                request.signed_headers(),
                "POST",
                &request.path,
                &request.body,
                now,
            )
            .ok_or_else(auth)?;
        self.limit(
            &peer,
            &request.signature,
            request.timestamp.parse().map_err(|_| auth())?,
        )?;
        if request.path == "/v1/discover" {
            return Ok(json!({"service":"shellfox","protocolVersion":1}));
        }
        // HMAC and peer validation precede both JSON decoders.
        let (client, command) = if request.path == "/v1/request" {
            let envelope: Envelope =
                serde_json::from_slice(&request.body).map_err(|_| invalid())?;
            let command: RemoteCommand = serde_json::from_value(
                json!({"method":envelope.method,"payload":envelope.payload}),
            )
            .map_err(|_| invalid())?;
            command.validate().map_err(|_| invalid())?;
            (envelope.client_id, Some(command))
        } else {
            let identity: Identity =
                serde_json::from_slice(&request.body).map_err(|_| invalid())?;
            (identity.client_id, None)
        };
        if client.is_nil() {
            return Err(invalid());
        }
        let id = format!("{}/{}", peer.id, client);
        if request.path == "/v1/disconnect" {
            self.approvals
                .withdraw_async(peer.clone(), client, now)
                .await?;
            let previous = self
                .limits
                .lock()
                .map_err(|_| invalid())?
                .views
                .get_mut(&id)
                .map(|view| {
                    view.paired = false;
                    (view.epoch, view.actor.clone())
                });
            if let Some((epoch, actor)) = previous {
                self.host
                    .request(OwnerCommand::Disconnect { actor })
                    .await?;
                self.remove_view(&id, epoch)?;
            }
            return Ok(json!({"disconnected":true}));
        }
        let mut actor = self
            .approvals
            .authorize_async(peer.clone(), client, now)
            .await?;
        if let Some(command) = &command {
            crate::policy::authorize(actor.permission, command)?;
        }
        if request.path == "/v1/pair" {
            let epoch = {
                let mut limits = self.limits.lock().map_err(|_| invalid())?;
                if limits.views.len() >= 32 && !limits.views.contains_key(&id) {
                    return Err(invalid());
                }
                limits.epoch = limits.epoch.checked_add(1).ok_or_else(invalid)?;
                let epoch = limits.epoch;
                actor.view_epoch = epoch;
                limits.views.insert(
                    id.clone(),
                    View {
                        actor: actor.clone(),
                        touched: now,
                        epoch,
                        paired: false,
                        polling: false,
                        requests: 0,
                    },
                );
                epoch
            };
            let result = self
                .host
                .request(OwnerCommand::Pair {
                    actor: actor.clone(),
                })
                .await;
            actor.validate_async().await?;
            let mut limits = self.limits.lock().map_err(|_| invalid())?;
            let view = limits
                .views
                .get_mut(&id)
                .filter(|v| v.epoch == epoch)
                .ok_or_else(auth)?;
            view.paired = result.is_ok();
            result?;
            return Ok(json!({"paired":true,"terminalControl":"exclusive-v1"}));
        }
        let polling = request.path == "/v1/events";
        let epoch = {
            let mut limits = self.limits.lock().map_err(|_| invalid())?;
            let view = limits
                .views
                .get_mut(&id)
                .filter(|v| v.paired)
                .ok_or_else(auth)?;
            if (polling && view.polling) || (!polling && view.requests >= 8) {
                return Err(invalid());
            }
            if view.actor.approval_epoch != actor.approval_epoch {
                return Err(auth());
            }
            actor = view.actor.clone();
            view.touched = now;
            if polling {
                view.polling = true;
            } else {
                view.requests += 1;
            }
            view.epoch
        };
        let result = if polling {
            self.poll(&id, epoch, actor.clone()).await
        } else {
            let command = command.ok_or_else(invalid)?;
            self.host
                .request(OwnerCommand::Execute {
                    actor: actor.clone(),
                    command,
                })
                .await
        };
        {
            let mut limits = self.limits.lock().map_err(|_| invalid())?;
            let view = limits
                .views
                .get_mut(&id)
                .filter(|v| v.epoch == epoch)
                .ok_or_else(auth)?;
            if polling {
                view.polling = false;
            } else {
                view.requests = view.requests.saturating_sub(1);
            }
        }
        actor.validate_async().await?;
        result
    }
    async fn poll(&self, id: &str, epoch: u64, actor: Actor) -> Result<Value, AppError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            actor.validate_async().await?;
            {
                let limits = self.limits.lock().map_err(|_| invalid())?;
                if !limits.views.get(id).is_some_and(|v| v.epoch == epoch) {
                    return Err(auth());
                }
            }
            let value = self
                .host
                .request(OwnerCommand::Poll {
                    actor: actor.clone(),
                })
                .await?;
            let response: shellfox_protocol::PollResponse =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            response.validate().map_err(|_| invalid())?;
            if !response.events.is_empty() || tokio::time::Instant::now() >= deadline {
                return Ok(value);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    fn limit(&self, peer: &PeerIdentity, signature: &str, timestamp: u64) -> Result<(), AppError> {
        let mut limits = self.limits.lock().map_err(|_| invalid())?;
        // Sample under the limit lock so concurrent HMAC work cannot look like
        // a backwards clock. Recheck the signed window at admission time.
        let now = (self.clock)();
        if now.abs_diff(timestamp) > 30_000 {
            return Err(auth());
        }
        // Never make an expired signed mutation reusable after a wall-clock
        // rollback. A new backend lifetime also rotates the helper secret.
        if limits.last_clock.is_some_and(|previous| now < previous) {
            return Err(error(
                ErrorCode::AuthFailed,
                "Host clock moved backwards. Wait for clock correction before reconnecting.",
                false,
            ));
        }
        limits.last_clock = Some(now);
        limits.replay.retain(|_, until| *until >= now);
        limits
            .rates
            .retain(|_, (since, _)| now.saturating_sub(*since) <= 1000);
        if limits.replay.contains_key(signature)
            || limits.replay.len() >= 8192
            || limits.rates.len() >= 2000
        {
            return Err(invalid());
        }
        // Include future timestamps still allowed by the helper clock window.
        limits
            .replay
            .insert(signature.into(), now.saturating_add(60_000));
        let rate = limits.rates.entry(peer.id.clone()).or_insert((now, 0));
        rate.1 = rate.1.saturating_add(1);
        if rate.1 > 240 {
            return Err(error(
                ErrorCode::Validation,
                "Remote request rate exceeded.",
                true,
            ));
        }
        Ok(())
    }
}
