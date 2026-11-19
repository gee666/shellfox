//! Actual, transient Linux requests. Run only in the original gated pidfd runner.
#![cfg(target_os = "linux")]
#[path = "support/power_linux_prerequisites.rs"]
mod power_linux_prerequisites;
use anyhow::{ensure, Context, Result};
use shellfox_service::power::{PowerManager, PowerStatus};

#[test]
fn linux_real_primary_logind_lease_is_confirmed_and_released() -> Result<()> {
    let mut manager = PowerManager::new();
    ensure!(manager.status() == PowerStatus::Inactive);
    let acquired = manager.set(true);
    let reason = manager.partial_reason().map(str::to_owned);
    // Release before asserting acquisition details, including unavailable-platform
    // failures. This test never leaves a successful request intentionally enabled.
    let released = manager.set(false);
    eprintln!(
        "Owned Linux power acquire={acquired:?}, partial_reason={reason:?}, release={released:?}"
    );
    ensure!(released.context("Actual Linux inhibitor release failed")? == PowerStatus::Inactive);
    let logind_error = reason
        .as_deref()
        .or_else(|| acquired.as_ref().err().map(|error| error.0.as_str()))
        .and_then(|details| {
            details
                .split("; ")
                .find_map(|detail| detail.strip_prefix("System/lid inhibitors unavailable: "))
        });
    if let Some(error) = logind_error {
        if power_linux_prerequisites::inhibitor_unavailable(error) {
            eprintln!("SKIP primary logind lease: {error}");
            return Ok(());
        }
        anyhow::bail!("Actual logind inhibitor acquisition failed: {error}");
    }
    let status = acquired.context("Actual Linux inhibitor acquisition failed")?;
    ensure!(matches!(status, PowerStatus::Active | PowerStatus::Partial));
    Ok(())
}
