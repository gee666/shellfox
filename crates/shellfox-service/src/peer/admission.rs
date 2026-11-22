//! Binds remote document I/O to the Manager binding that admitted the source.
//! UI selection generations are cancellation hints, never file authority.
use super::consumer::{reply, PeerConnection};
use anyhow::{Result, anyhow, ensure};
use shellfox_client::manager_v2::ManagerBindingWitness;
use shellfox_protocol::{AuthenticatedPeerId, OutboundTransport, manager_v2 as m};
#[derive(Clone)]
pub struct SourceAdmission {
    peer: AuthenticatedPeerId,
    witness: ManagerBindingWitness,
}
impl SourceAdmission {
    pub fn peer(&self) -> &AuthenticatedPeerId {
        &self.peer
    }
    /// Bounded infallible local publication only. No awaits, I/O, blocking or
    /// dropping native engines under the actual-client -> SDK -> owner fence.
    pub fn commit_if_current<R>(&self, operation: impl FnOnce() -> R) -> Result<R> {
        self.witness
            .commit_if_current(operation)
            .map_err(|error| anyhow!(error))
    }
    pub fn check(&self) -> Result<()> {
        self.commit_if_current(|| ())
    }
}
impl<T: OutboundTransport> PeerConnection<T> {
    pub fn admit_source_with(&self, witness: &ManagerBindingWitness) -> Result<SourceAdmission> {
        let client = self.client();
        let current = client.binding_witness().map_err(|error| anyhow!(error))?;
        ensure!(
            current.same_origin(witness),
            "Source belongs to another or retired Manager binding."
        );
        witness
            .commit_if_current(|| SourceAdmission {
                peer: client.peer().clone(),
                witness: witness.clone(),
            })
            .map_err(|error| anyhow!(error))
    }
    pub async fn execute_admitted<R: m::Request>(
        &self,
        source: &SourceAdmission,
        request: &R,
    ) -> Result<R::Response> {
        reply(self.execute_admitted_reply(source, request).await?)
    }

    pub async fn execute_admitted_reply<R: m::Request>(
        &self,
        source: &SourceAdmission,
        request: &R,
    ) -> Result<m::Reply<R::Response>> {
        let client = self.client();
        ensure!(
            source.peer == *client.peer(),
            "Document admission belongs to another peer."
        );
        source.check()?;
        let response = client
            .execute_with_witness(&source.witness, request)
            .await
            .map_err(|error| anyhow!(error))?;
        source.check()?;
        Ok(response)
    }
}
