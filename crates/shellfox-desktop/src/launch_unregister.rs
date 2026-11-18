//! Launch-registration metadata retires with the SAME confirmed Service owner.
use crate::{
    app_host::AppHost,
    host_client::{HostClient, HostEndpointStartupOutcome},
};
use shellfox_service::desktop_launch::DesktopClientToken;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Unregistered {
    Receipt,
    RetiredWithOriginalHostStop,
}
fn retired(outcome: anyhow::Result<HostEndpointStartupOutcome>) -> bool {
    matches!(outcome, Ok(HostEndpointStartupOutcome::NoOwnerClean))
}
pub(super) async fn finish(
    client: &HostClient,
    token: DesktopClientToken,
    original: &AppHost,
) -> anyhow::Result<Unregistered> {
    // Serialize with in-flight last-window Stop, not with arbitrary error text.
    if retired(original.startup_outcome_after_operations().await) {
        return Ok(Unregistered::RetiredWithOriginalHostStop);
    }
    match client.desktop_unregister(token).await {
        Ok(()) => Ok(Unregistered::Receipt),
        Err(error) => {
            // Stop may win AFTER the first observation but BEFORE native admission.
            if retired(original.startup_outcome_after_operations().await) {
                Ok(Unregistered::RetiredWithOriginalHostStop)
            } else {
                Err(error)
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_typed_original_no_owner_confirmation_supersedes_unregister() {
        assert!(retired(Ok(HostEndpointStartupOutcome::NoOwnerClean)));
        for state in [
            HostEndpointStartupOutcome::Ready,
            HostEndpointStartupOutcome::RetainedCleanup,
            HostEndpointStartupOutcome::ReadyLocalOnly {
                remote_unavailable:
                    shellfox_service::remote::RemoteUnavailableReason::NativeApprovalStoragePending,
            },
        ] {
            assert!(!retired(Ok(state)));
        }
        assert!(!retired(Err(anyhow::anyhow!(
            "NoOwnerClean: untrusted text"
        ))));
    }
}
