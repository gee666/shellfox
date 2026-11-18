use super::{ClientError, Status};
use crate::v2_common::{CommonClient, ObserverProfile};
use shellfox_protocol::{observer_v2 as o, *};
pub struct ObserverClient<T> {
    pub(super) inner: CommonClient<T, ObserverProfile>,
}
impl<T: OutboundTransport> ObserverClient<T> {
    pub fn new(peer: AuthenticatedPeerId, client: o::Id, transport: T) -> Self {
        Self {
            inner: CommonClient::new(peer, client, transport),
        }
    }
    pub fn status(&self) -> Status {
        self.inner.status()
    }
    pub fn binding(&self) -> Option<o::Binding> {
        self.inner.binding()
    }
    pub fn peer(&self) -> &AuthenticatedPeerId {
        self.inner.peer()
    }
    pub fn reconnect(&self, client: o::Id) {
        self.inner.reconnect(client)
    }
    pub async fn execute<R: o::Request>(
        &self,
        request: &R,
    ) -> Result<o::Reply<R::Response>, ClientError> {
        request
            .validate_wire()
            .map_err(|_| ClientError::InvalidRequest)?;
        if !R::ALLOWED {
            return Err(ClientError::ProfileDenied);
        }
        Ok(match self.inner.request(R::METHOD, request).await? {
            Ok(v) => o::Reply::Success(v),
            Err(e) => o::Reply::Failure(e),
        })
    }
    pub async fn poll(&self) -> Result<o::Reply<o::PollResponse>, ClientError> {
        Ok(match self.inner.poll().await? {
            Ok(v) => o::Reply::Success(v),
            Err(e) => o::Reply::Failure(e),
        })
    }
}
