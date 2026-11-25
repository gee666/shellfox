//! `host status` and `host keep-awake` in 0.9's shapes (src/host/status.ts).
use crate::output::CliError;
use anyhow::Result;
use serde_json::{json, Value};
use shellfox_service::{
    local::{DesktopReply, DesktopRequest, LocalClient},
    power::{PowerReport, PowerStatus},
};

fn unexpected() -> anyhow::Error {
    anyhow::anyhow!("Unexpected host response.")
}

/// `{mode, pid, startedAt, capabilities}`. As in 0.9, only a desktop host
/// offers desktop actions and Administrator terminals.
pub async fn status(client: &LocalClient) -> Result<Value> {
    let DesktopReply::Status(status) = client.request(&DesktopRequest::Status).await? else {
        return Err(unexpected());
    };
    let administrator = status.desktop
        && match client.request(&DesktopRequest::TerminalProfiles).await? {
            DesktopReply::TerminalProfiles(catalog) => serde_json::to_value(&catalog.profiles)?
                ["profiles"]
                .as_array()
                .is_some_and(|profiles| {
                    profiles
                        .iter()
                        .any(|p| p["available"] == true && p["elevation"] == "administrator")
                }),
            _ => return Err(unexpected()),
        };
    let keep_awake = client.power_status().await.is_ok();
    Ok(json!({
        "mode": if status.desktop { "desktop" } else { "headless" },
        "pid": status.pid,
        "startedAt": status.started_at,
        "capabilities": {
            "keepAwake": keep_awake,
            "desktopActions": status.desktop,
            "administrator": administrator,
        },
    }))
}

/// 0.9's `{active}`; a change the host couldn't confirm is an error.
pub async fn keep_awake(client: &LocalClient, active: Option<bool>) -> Result<Value> {
    let report: PowerReport = match active {
        None => client.power_status().await?,
        Some(active) => client.set_power(active).await?,
    };
    if active.is_some() && (report.pending || report.error.is_some()) {
        return Err(CliError::coded(
            "NATIVE_UNAVAILABLE",
            report.error.unwrap_or_else(|| {
                "Keep awake change is not confirmed; check host keep-awake and retry.".into()
            }),
            false,
        ));
    }
    let on = matches!(report.status, PowerStatus::Active | PowerStatus::Partial);
    Ok(json!({ "active": on }))
}
