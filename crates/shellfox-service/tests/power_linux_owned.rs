//! Actual, transient Linux requests. Run only in the original gated pidfd runner.
#![cfg(target_os = "linux")]
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
    let status = acquired.context("Actual Linux inhibitor acquisition failed")?;
    ensure!(released.context("Actual Linux inhibitor release failed")? == PowerStatus::Inactive);
    ensure!(matches!(status, PowerStatus::Active | PowerStatus::Partial));
    ensure!(
        reason
            .as_deref()
            .is_none_or(|reason| !reason.contains("System/lid inhibitors unavailable:")),
        "Only display inhibition was available; this is not primary logind acceptance: {reason:?}"
    );
    Ok(())
}
