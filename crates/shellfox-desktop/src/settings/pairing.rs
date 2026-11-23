use super::{api::Connection, *};
use shellfox_protocol::CancellationToken;
use shellfox_service::remote::native::{ConnectResult, PairingPermit};
use std::{collections::HashSet, future::Future, time::Instant};

const INTERVAL: Duration = Duration::from_secs(1);
// End before the host's five-minute denial tombstone can expire.
const LIFETIME: Duration = Duration::from_secs(120);

#[derive(Default)]
pub(super) struct Scope {
    pub generation: u64,
    lifetime: CancellationToken,
    stopped: HashSet<String>,
}
impl Scope {
    pub(super) fn stopped(&self, id: &str) -> bool {
        self.stopped.contains(id)
    }
    pub(super) fn retain_attempts(&mut self, state: &api::RemoteState) {
        self.stopped
            .retain(|id| state.attempts.iter().any(|attempt| &attempt.id == id));
    }
    fn record_failure(&mut self, generation: u64, id: String) -> bool {
        generation == self.generation && self.stopped.insert(id)
    }
    fn dismiss(&mut self) {
        self.lifetime.cancel();
        self.lifetime = CancellationToken::new();
        self.generation += 1;
    }
    fn lifetime(&self, generation: u64) -> Option<CancellationToken> {
        (generation == self.generation).then(|| self.lifetime.child_token())
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}

impl SettingsPanel {
    pub(super) fn stop_pairing(&mut self) {
        self.remote.pairing_scope.dismiss();
    }

    pub(super) fn watch_pairing(
        &mut self,
        peer: String,
        state: &api::RemoteState,
        result: ConnectResult,
        started: Instant,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        if !self.visible || self.operations.frozen() {
            return;
        }
        let Some(lifetime) = self.remote.pairing_scope.lifetime(generation) else {
            return;
        };
        let Some(watch) = Watch::new(state, peer, result, started, lifetime) else {
            return;
        };
        let Some(services) = self.services.clone() else {
            return;
        };
        cx.spawn(async move |panel, cx| {
            let executor = cx.background_executor().clone();
            let result = watch
                .run(
                    services.as_ref(),
                    |delay| executor.timer(delay),
                    |permit| {
                        panel
                            .update(cx, |panel, _| {
                                (panel.visible
                                    && !panel.operations.frozen()
                                    && permit.active_at(Instant::now()))
                                .then(|| panel.operations.begin())
                            })
                            .ok()
                            .flatten()
                    },
                )
                .await;
            if result.is_err() {
                let _ = panel.update(cx, |panel, cx| {
                    if panel
                        .remote
                        .pairing_scope
                        .record_failure(generation, watch.id)
                    {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
}

pub(crate) struct Watch {
    peer: String,
    id: String,
    permit: PairingPermit,
}

#[derive(Debug, Eq, PartialEq)]
enum Poll {
    Finished,
    Stopped,
    Waiting,
    Continue,
}

pub(crate) struct CheckFailed;

impl Watch {
    pub(crate) async fn run<Timer: Future<Output = ()>, Lease>(
        &self,
        services: &dyn api::SettingsServices,
        timer: impl Fn(Duration) -> Timer,
        mut begin: impl FnMut(&PairingPermit) -> Option<Lease>,
    ) -> Result<(), CheckFailed> {
        let _cancel = self.permit.lifetime.clone().drop_guard();
        loop {
            tokio::select! {
                biased;
                _ = self.permit.lifetime.cancelled() => return Ok(()),
                _ = timer(INTERVAL.min(self.remaining())) => {}
            }
            if !self.permit.active_at(Instant::now()) {
                return Ok(());
            }
            let state = self
                .request(services, &timer, api::Request::RemoteState)
                .await?;
            let Some(state) = state else {
                return Ok(());
            };
            match self.plan(&state, Instant::now()) {
                Poll::Finished | Poll::Stopped => return Ok(()),
                Poll::Waiting => continue,
                Poll::Continue => {}
            }
            let Some(_lease) = begin(&self.permit) else {
                return Ok(());
            };
            let request = api::Request::ContinuePair {
                id: self.id.clone(),
                permit: self.permit.clone(),
            };
            if self.request(services, &timer, request).await?.is_none() {
                return Ok(());
            }
        }
    }

    async fn request<Timer: Future<Output = ()>>(
        &self,
        services: &dyn api::SettingsServices,
        timer: impl Fn(Duration) -> Timer,
        request: api::Request,
    ) -> Result<Option<api::RemoteState>, CheckFailed> {
        let result = tokio::select! {
            biased;
            _ = self.permit.lifetime.cancelled() => return Ok(None),
            _ = timer(self.remaining()) => return Ok(None),
            result = services.request(request) => result,
        };
        self.accept(result).map(Some)
    }
    pub(crate) fn new(
        state: &api::RemoteState,
        peer: String,
        result: ConnectResult,
        started: Instant,
        lifetime: shellfox_protocol::CancellationToken,
    ) -> Option<Self> {
        let ConnectResult::Created(id) = result else {
            return None;
        };
        let watch = Self {
            peer,
            id: id.to_string(),
            permit: PairingPermit {
                deadline: started + LIFETIME,
                lifetime,
            },
        };
        has_attempt(state, &watch.peer, &watch.id).then_some(watch)
    }
    fn remaining(&self) -> Duration {
        self.permit
            .deadline
            .saturating_duration_since(Instant::now())
    }
    fn accept(
        &self,
        reply: anyhow::Result<api::Response>,
    ) -> Result<api::RemoteState, CheckFailed> {
        if let Ok(api::Response::Remote(state)) = reply {
            return Ok(state);
        }
        // Don't replay an uncertain continuation or admit its late result.
        self.permit.lifetime.cancel();
        Err(CheckFailed)
    }
    fn plan(&self, state: &api::RemoteState, now: Instant) -> Poll {
        if !self.permit.active_at(now) {
            return Poll::Stopped;
        }
        if !state.enabled || !has_attempt(state, &self.peer, &self.id) {
            return Poll::Finished;
        }
        match state
            .peers
            .iter()
            .find(|entry| entry.id == self.peer)
            .map(|entry| entry.connection)
        {
            Some(Connection::AwaitingApproval) if state.status == api::NetworkStatus::Online => {
                Poll::Continue
            }
            Some(
                Connection::AwaitingApproval | Connection::Connecting | Connection::Reconnecting,
            ) => Poll::Waiting,
            _ => Poll::Finished,
        }
    }
}

fn has_attempt(state: &api::RemoteState, peer: &str, id: &str) -> bool {
    state.attempts.iter().any(|attempt| {
        attempt.id == id && matches!(&attempt.kind, api::RemoteAttemptKind::Connect { peer_id } if peer_id == peer)
    })
}

#[cfg(test)]
mod tests;
