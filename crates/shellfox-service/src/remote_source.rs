//! Single-owner, initially pending sanitized watch binding for async projection.
use super::*;
use crate::activity_integration::{read_source, source_is_current, SourceFacts, SourceFactsWatch};
use std::sync::Arc;
#[cfg(test)]
#[path = "remote_source_tests.rs"]
mod tests;

/// Discovery/projection receives immutable owner evidence, not Host/Runtime or
/// execution rights. Return unavailable for facts that have no actual evidence.
/// Active phase alone is not an agent observation; activity_available=false
/// cannot prove idle. Missing tab creation evidence is never session creation.
pub trait SourceFactsProjector: Send + Sync {
    fn project(
        &self,
        source: Arc<SourceFacts>,
        request: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_>;
    fn project_capabilities(
        &self,
        _source: Arc<SourceFacts>,
        _request: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_> {
        Box::pin(async {
            Err(capability_unavailable(
                "independent native capability projection",
            ))
        })
    }
}
/// The boot hook's structural type. No dependency on a GUI or a mutable owner.
pub type SourceFactsInstaller = Arc<dyn Fn(SourceFactsWatch) -> anyhow::Result<()> + Send + Sync>;
pub struct SourceFactsProviderSlot {
    binding: tokio::sync::watch::Sender<Option<SourceFactsWatch>>,
    projector: Arc<dyn SourceFactsProjector>,
}
impl SourceFactsProviderSlot {
    pub fn new(projector: Arc<dyn SourceFactsProjector>) -> Self {
        Self {
            binding: tokio::sync::watch::channel(None).0,
            projector,
        }
    }
    /// Install once after the actual owner activation ACK. An unpopulated watch
    /// remains pending; closed/replacement watches cannot impersonate that owner.
    pub fn install(&self, source: SourceFactsWatch) -> anyhow::Result<()> {
        anyhow::ensure!(
            source.has_changed().is_ok(),
            "Owned source facts watch is closed."
        );
        let mut occupied = false;
        self.binding.send_if_modified(|binding| {
            if binding.is_some() {
                occupied = true;
                false
            } else {
                *binding = Some(source.clone());
                true
            }
        });
        anyhow::ensure!(!occupied, "Owned source facts watch was already installed.");
        Ok(())
    }
    pub fn installer(self: &Arc<Self>) -> SourceFactsInstaller {
        let slot = self.clone();
        Arc::new(move |source| slot.install(source))
    }
    fn capture(
        &self,
        request: &ManagerFactsRequest,
    ) -> Result<(SourceFactsWatch, Arc<SourceFacts>), AppError> {
        if request.cancelled.is_cancelled() {
            return Err(failure(
                ErrorCode::NativeUnavailable,
                "Owner facts request was cancelled.",
            ));
        }
        let watch =
            self.binding.borrow().clone().ok_or_else(|| {
                capability_unavailable("owned source facts watch is not installed")
            })?;
        // Clone and release BOTH watch borrows before discovery/permissions await.
        let source = read_source(&watch).ok_or_else(|| {
            capability_unavailable("owned source facts are pending or unavailable")
        })?;
        if !source.matches_request(request) {
            return Err(stale());
        }
        Ok((watch, source))
    }
}
impl SourceFactsProviderSlot {
    fn collect_mode(
        &self,
        request: ManagerFactsRequest,
        capabilities_only: bool,
    ) -> ManagerFactsFuture<'_> {
        Box::pin(async move {
            let (watch, source) = self.capture(&request)?;
            let result = tokio::select! {
                _=request.cancelled.cancelled()=>Err(failure(ErrorCode::NativeUnavailable,"Owner facts request was cancelled.")),
                _=source.cancelled.cancelled()=>Err(failure(ErrorCode::NativeUnavailable,"Owned source facts were cancelled.")),
                result=async { if capabilities_only { self.projector.project_capabilities(source.clone(),request.clone()).await } else { self.projector.project(source.clone(),request.clone()).await } }=>result,
            };
            if request.cancelled.is_cancelled() || source.cancelled.is_cancelled() {
                return Err(failure(
                    ErrorCode::NativeUnavailable,
                    "Owned source facts were cancelled.",
                ));
            }
            if !source_is_current(&watch, &source) || !source.matches_request(&request) {
                return Err(stale());
            }
            result
        })
    }
}
impl ManagerFactsProvider for SourceFactsProviderSlot {
    fn collect(&self, request: ManagerFactsRequest) -> ManagerFactsFuture<'_> {
        self.collect_mode(request, false)
    }
    fn collect_capabilities(&self, request: ManagerFactsRequest) -> ManagerFactsFuture<'_> {
        self.collect_mode(request, true)
    }
}
fn stale() -> AppError {
    failure(ErrorCode::TargetLost,"Owned snapshot/settings/runtime facts changed during collection. Refresh explicitly; no mutation was executed.")
}
