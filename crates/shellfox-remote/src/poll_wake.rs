use std::sync::Arc;
use tokio::sync::Notify;

/// Wakes one paired view's pending `/v1/events` long-poll.
///
/// The service owner calls [`PollWake::notify`] whenever the view's next poll
/// would return events. A wake with no waiting poll is kept as a single permit,
/// so a wake that lands between the owner poll and the wait is never lost.
#[derive(Clone, Default)]
pub struct PollWake(Arc<Notify>);

impl PollWake {
    pub fn notify(&self) {
        self.0.notify_one();
    }
    pub(crate) async fn notified(&self) {
        self.0.notified().await;
    }
}
