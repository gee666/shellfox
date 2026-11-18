//! Secondary-process launch-only path. No rendering client or Host is constructed.
use crate::options::Options;
use anyhow::{Context, Result};
use shellfox_service::desktop_launch::LaunchRequestKind;

pub(crate) fn forward_existing(options: &Options) -> Result<bool> {
    let Some(owner) = options.launch_owner else {
        return Ok(false);
    };
    let config = options.endpoint.as_ref().context("Launch-only handoff requires the trusted installed helper configuration; no Host or UI was started")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        // This entry represents an existing request ID. Missing/ambiguous receipts
        // never authorize another creation, submission to a replacement owner, or cold start.
        crate::host_client::handoff_existing(
            config,
            options.launch.clone(),
            LaunchRequestKind::Known,
            owner,
        )
        .await
        .context("Launch-only handoff did not complete; no Host or UI was started")
    })?;
    Ok(true)
}
