use super::{ClientError, ObserverClient};
pub use crate::v2_common::{DiscoveryOutcome, FallbackPolicy};
use shellfox_protocol::{observer_v2 as o, OutboundTransport};
impl<T: OutboundTransport> ObserverClient<T> {
    pub async fn discover(&self, policy: FallbackPolicy) -> Result<DiscoveryOutcome, ClientError> {
        self.inner.discover(policy).await
    }
    pub async fn pair(&self, attempt: o::Id) -> Result<o::Reply<o::PairSelection>, ClientError> {
        Ok(match self.inner.pair(attempt).await? {
            Ok(v) => o::Reply::Success(v),
            Err(e) => o::Reply::Failure(e),
        })
    }
    pub async fn disconnect(&self) -> Result<o::Reply<o::Disconnected>, ClientError> {
        Ok(match self.inner.disconnect().await? {
            Ok(v) => o::Reply::Success(v),
            Err(e) => o::Reply::Failure(e),
        })
    }
}
