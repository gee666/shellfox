use crate::{
    args::{Args, Command},
    attach,
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use shellfox_core::Snapshot;
use shellfox_service::{
    experiment::{ExperimentRequest, ExperimentResponse},
    local::{self, LocalClient},
};

/// The headless owner stays outside block_on/catch, just like the desktop thread.
pub fn serve(args: Args, runtime: &tokio::runtime::Runtime, ready: impl Fn()) -> Result<i32> {
    let remote = args
        .application
        .remote_options(std::env::current_exe()?, false)?;
    let mut bootstrap =
        local::LocalBootstrap::plan(&args.data_dir, &args.helper, args.helper_sha256)?;
    let clean = std::cell::Cell::new(false);
    let readiness_reported = std::cell::Cell::new(false);
    let report = |report: local::LocalBootReport| {
        clean.set(report.status == local::LocalBootStatus::NoOwnerClean);
        match report.status {
            local::LocalBootStatus::ReadyLocalOnly { remote_unavailable } if !readiness_reported.replace(true) => {
                match remote_unavailable {
                    shellfox_service::remote::RemoteUnavailableReason::NativeApprovalStoragePending => eprintln!("Local host ready. Remote unavailable: native approval storage review is pending."),
                }
                ready();
            }
            local::LocalBootStatus::Ready if !readiness_reported.replace(true) => ready(),
            _ => {}
        }
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(bootstrap.run_remote(remote, |_| {}, &report))
    }))
    .unwrap_or_else(|_| {
        Err(bootstrap.retain_error(anyhow::anyhow!(
            "Headless host execution panicked; original cleanup retained."
        )))
    });
    const CLEANUP_ATTEMPTS: u32 = 5;
    for attempt in 1..=CLEANUP_ATTEMPTS {
        if clean.get() {
            return result.map(|()| 0);
        }
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.block_on(bootstrap.cleanup(&report));
        }))
        .is_err();
        if panicked && attempt < CLEANUP_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    if clean.get() {
        return result.map(|()| 0);
    }
    let cleanup = anyhow::anyhow!(
        "Host cleanup did not finish after {CLEANUP_ATTEMPTS} attempts; the next host run retries it."
    );
    Err(match result {
        Err(error) => error.context(cleanup),
        Ok(()) => cleanup,
    })
}

pub async fn run(args: Args) -> Result<i32> {
    // Reject redirected input before claiming or even starting a host.
    if requires_terminal(&args.command) {
        attach::require_terminal()?;
    }
    let mut client = LocalClient::connect(&args.data_dir, &args.helper, args.helper_sha256).await?;
    let stop = matches!(&args.command, Command::Request(ExperimentRequest::Stop {}));
    let result = match args.command {
        Command::Automation(command) => crate::automation::run(&client, command).await,
        command => run_connected(&mut client, command).await.map(|()| 0),
    };
    if !stop {
        // Disconnect releases the controller slot, not the host's terminal.
        let _ = client.disconnect().await;
    }
    result
}

fn requires_terminal(command: &Command) -> bool {
    match command {
        Command::Attach { .. } => true,
        _ => false,
    }
}

async fn run_connected(client: &mut LocalClient, command: Command) -> Result<()> {
    match command {
        Command::Serve => unreachable!("serve returned before connecting"),
        Command::Network(command) => crate::remote_command::run(client, command).await,
        Command::Power(request) => {
            let changing = matches!(request, shellfox_service::power::PowerRequest::Set { .. });
            let report = match request {
                shellfox_service::power::PowerRequest::Status {} => client.power_status().await?,
                shellfox_service::power::PowerRequest::Set { active } => {
                    client.set_power(active).await?
                }
            };
            print_json(&report)?;
            if changing && (report.pending || report.error.is_some()) {
                bail!(
                    "Local host keep-awake change is not confirmed; inspect the returned report and retry OFF if release failed."
                );
            }
            Ok(())
        }
        Command::Automation(_) => unreachable!("automation returned before metadata routing"),
        Command::DesktopLaunch(intent) => print_json(
            &client
                .launch_existing(
                    intent,
                    shellfox_service::desktop_launch::LaunchRequestKind::Fresh,
                )
                .await?,
        ),
        Command::Sessions => {
            let snapshot = snapshot(client).await?;
            print_json(&snapshot.sessions)
        }
        Command::Tabs(session) => {
            let snapshot = snapshot(client).await?;
            let tabs: Vec<_> = if let Some(id) = session {
                snapshot
                    .sessions
                    .iter()
                    .find(|session| session.id == id)
                    .context("Session not found.")?
                    .tabs
                    .iter()
                    .collect()
            } else {
                snapshot
                    .sessions
                    .iter()
                    .flat_map(|session| &session.tabs)
                    .collect()
            };
            print_json(&tabs)
        }
        Command::Request(request) => {
            let settings = matches!(&request, ExperimentRequest::Settings { .. });
            let stop = matches!(&request, ExperimentRequest::Stop {});
            match client.execute(request).await? {
                ExperimentResponse::Done {} if stop => {
                    print_json(&serde_json::json!({ "stopped": true }))
                }
                ExperimentResponse::Settings(value) if settings => print_json(&value),
                ExperimentResponse::Snapshot(value) if !settings && !stop => print_json(&value),
                _ => bail!("Unexpected service response to metadata command."),
            }
        }
        Command::Attach { tab, takeover } => attach::run(client, tab, takeover).await,
    }
}

async fn snapshot(client: &mut LocalClient) -> Result<Snapshot> {
    match client.execute(ExperimentRequest::Snapshot {}).await? {
        ExperimentResponse::Snapshot(snapshot) => Ok(snapshot),
        _ => bail!("Unexpected service snapshot response."),
    }
}

pub(crate) fn print_json(value: &impl Serialize) -> Result<()> {
    crate::output::print_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_attach_is_guarded_before_connection_and_serve_is_not() {
        assert!(requires_terminal(&Command::Attach {
            tab: shellfox_core::TabId::new(),
            takeover: false
        }));
        assert!(!requires_terminal(&Command::Serve));
    }
}
