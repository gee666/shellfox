//! Runs the 0.9.x command surface against the Rust host.
use crate::{
    legacy_host::LegacyHost,
    node_args::{data_dir, Action, Invocation},
    output::{print_value, CliError},
};
use anyhow::{Context, Result};
use shellfox_core::{AddTab, CreateSession, ProfileId, SessionId, Snapshot};
use shellfox_service::{
    experiment::{ExperimentRequest, ExperimentResponse},
    local::LocalClient,
    remote::RemoteRequest,
};
use std::{
    io::{IsTerminal, Write},
    path::Path,
};

pub const HELP: &str = "Shellfox

Usage: shellfox [--user-data DIR] [--json] [--yes] COMMAND

Host
  host run | host status | host stop
  host keep-awake [on|off]
  host service install [--startup] | host service uninstall
Sessions (session, sessions, ss)
  session list | create [PATH] [--title TITLE] | inspect ID
  session rename ID TITLE | delete ID
Terminals (terminals, tm)
  terminals list [SESSION] | create SESSION [--title T] [--profile P]
  terminals rename SESSION TAB TITLE | close TAB | attach TAB
Automation
  run [--cwd PATH] [--title T] [--request-id UUID] [--detach] -- EXE [ARG...]
  runs list | runs show ID | runs stop ID
Network
  network status | enable [--hostname NAME] | disable
  network login [--auth-key-stdin]
  machines list | connect ID | disconnect ID | forget ID
  approvals list | approve ID | reject ID | revoke ID
Other
  settings read | settings update --input JSON|@FILE|-
  ssh [NAME | --profile NAME]
  update [--check] | version | help

Destructive commands need --yes in noninteractive or --json mode.
Ctrl+] detaches from an attached terminal.";

pub fn run(invocation: Invocation, watch: Option<crate::parent_watch::Watch>) -> Result<i32> {
    let Invocation {
        user_data,
        json,
        yes,
        action,
    } = invocation;
    let data = data_dir(&user_data);
    let runtime = || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
    };
    match action {
        Action::Help => print_value(&HELP)?,
        Action::Version => print_value(&env!("SHELLFOX_VERSION"))?,
        Action::Update { check } => {
            let result = runtime()?.block_on(crate::headless_update::run(check))?;
            if json {
                print_value(&result)?
            } else {
                print_value(&result.message)?
            }
        }
        Action::HostRun => {
            if let Some(legacy) = runtime()?.block_on(LegacyHost::connect(&user_data)) {
                runtime()?.block_on(legacy.close());
                return Err(CliError::new("HOST_RUNNING", "A Shellfox 0.9 host still owns this data directory. Stop it with host stop --yes, or restart it through its startup manager.", false));
            }
            return host_run(&data, json, watch);
        }
        Action::HostStatus => runtime()?.block_on(async {
            if let Ok(client) = connect(&data).await {
                let _ = client.disconnect().await;
                return print_value(&serde_json::json!({ "running": true }));
            }
            let legacy = LegacyHost::connect(&user_data)
                .await
                .ok_or_else(unavailable)?;
            let status = legacy.status().await;
            legacy.close().await;
            print_value(&status?)
        })?,
        Action::HostStop => {
            confirm(json, yes, "Stop the host and all its terminals")?;
            runtime()?.block_on(async {
                if let Ok(client) = connect(&data).await {
                    client.execute(ExperimentRequest::Stop {}).await?;
                } else {
                    let legacy = LegacyHost::connect(&user_data)
                        .await
                        .ok_or_else(unavailable)?;
                    legacy.stop().await?;
                }
                print_value(&serde_json::json!({ "stopped": true }))
            })?
        }
        Action::Service { install, startup } => {
            if !install {
                confirm(json, yes, "Remove host startup and stop the host")?;
            }
            let message = crate::host_service::run(install, startup, &user_data, &data)?;
            print_value(&message)?;
        }
        Action::Host {
            words,
            confirm: what,
        } => {
            if let Some(what) = what {
                confirm(json, yes, &what)?;
            }
            let args = crate::args::packaged(data, &words)?;
            if let Some(watch) = watch {
                watch.spawn(|| std::process::exit(1));
            }
            return runtime()?.block_on(crate::commands::run(args));
        }
        action => runtime()?.block_on(connected(&data, action))?,
    }
    Ok(0)
}

async fn connected(data: &Path, action: Action) -> Result<()> {
    let client = connect(data).await?;
    let result = query(&client, action).await;
    let _ = client.disconnect().await;
    result
}

async fn query(client: &LocalClient, action: Action) -> Result<()> {
    match action {
        Action::CreateSession { cwd, title } => {
            let before = snapshot(client).await?;
            let input = CreateSession {
                request_id: SessionId::new(),
                title,
                cwd,
                profile_id: default_profile(&before),
            };
            let snapshot = mutate(client, ExperimentRequest::CreateSession { input }).await?;
            let created = snapshot
                .sessions
                .iter()
                .find(|s| !before.sessions.iter().any(|old| old.id == s.id));
            print_value(&created.context("The host did not return the new session.")?)
        }
        Action::CreateTab {
            session,
            title,
            profile,
        } => {
            let id: SessionId = session
                .parse()
                .map_err(|_| CliError::usage("A session UUID is required."))?;
            let before = snapshot(client).await?;
            let profile_id = match profile {
                Some(profile) => profile
                    .parse()
                    .map_err(|_| CliError::usage("Invalid profile ID."))?,
                None => default_profile(&before),
            };
            let input = AddTab {
                title,
                cwd: None,
                profile_id,
            };
            let snapshot = mutate(client, ExperimentRequest::AddTab { id, input }).await?;
            let old: Vec<_> = before.sessions.iter().flat_map(|s| &s.tabs).map(|t| t.id).collect();
            let created = snapshot
                .sessions
                .iter()
                .filter(|s| s.id == id)
                .flat_map(|s| &s.tabs)
                .find(|t| !old.contains(&t.id));
            print_value(&created.context("The host did not return the new terminal.")?)
        }
        Action::Inspect(id) => {
            let id: SessionId = id
                .parse()
                .map_err(|_| CliError::usage("A session UUID is required."))?;
            let snapshot = snapshot(client).await?;
            let session = snapshot.sessions.iter().find(|s| s.id == id);
            print_value(
                &session.ok_or_else(|| CliError::new("NOT_FOUND", "Session not found.", false))?,
            )
        }
        Action::Machines => {
            let state = client.remote_control(RemoteRequest::State).await?;
            let hosts: Vec<_> = state
                .peers
                .iter()
                .filter(|peer| peer.host_confirmed)
                .collect();
            print_value(&hosts)
        }
        Action::Approvals => {
            let state = client.remote_control(RemoteRequest::State).await?;
            print_value(
                &serde_json::json!({ "approvals": state.approvals, "pending": state.pending }),
            )
        }
        _ => unreachable!("handled before connecting"),
    }
}

fn default_profile(snapshot: &Snapshot) -> ProfileId {
    snapshot.preferences.default_profile_id.clone()
}
async fn snapshot(client: &LocalClient) -> Result<Snapshot> {
    mutate(client, ExperimentRequest::Snapshot {}).await
}
async fn mutate(client: &LocalClient, request: ExperimentRequest) -> Result<Snapshot> {
    match client.execute(request).await? {
        ExperimentResponse::Snapshot(snapshot) => Ok(snapshot),
        _ => anyhow::bail!("Unexpected host response."),
    }
}

pub async fn connect(data: &Path) -> Result<LocalClient> {
    let (helper, digest) = shellfox_service::app_paths::packaged_helper()?;
    LocalClient::connect(data, &helper, digest)
        .await
        .map_err(|_| unavailable())
}
fn unavailable() -> anyhow::Error {
    CliError::new(
        "HOST_UNAVAILABLE",
        "Host connection failed. Run shellfox host run with the same --user-data directory.",
        false,
    )
}

fn confirm(json: bool, yes: bool, what: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    if json || !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(CliError::usage(format!(
            "{what} requires --yes in noninteractive or JSON mode."
        )));
    }
    print!("{what}? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if matches!(answer.trim(), "y" | "Y" | "yes") {
        Ok(())
    } else {
        Err(CliError::new(
            "CANCELLED",
            "Cancelled. No changes made.",
            false,
        ))
    }
}

/// Foreground host. Readiness is one record; a lost launcher stops it cleanly.
fn host_run(data: &Path, json: bool, watch: Option<crate::parent_watch::Watch>) -> Result<i32> {
    create_private_dir(data)?;
    let args = crate::args::packaged(data.to_owned(), &["host".into(), "serve".into()])?;
    let started = crate::time_text::now_iso();
    if let Some(watch) = watch {
        let data = data.to_owned();
        watch.spawn(move || {
            let stop = async {
                let client = connect(&data).await?;
                client.execute(ExperimentRequest::Stop {}).await
            };
            let stopped = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map(|runtime| runtime.block_on(stop));
            if !matches!(stopped, Ok(Ok(_))) {
                std::process::exit(1);
            }
        });
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    crate::commands::serve(args, &runtime, || {
        let status = serde_json::json!({
            "mode": "headless", "pid": std::process::id(), "startedAt": started,
            "capabilities": { "keepAwake": true, "desktopActions": false, "administrator": false },
        });
        if !json {
            println!("Host running. Press Ctrl+C to stop.");
        }
        let _ = print_value(&status);
    })
}

fn create_private_dir(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(path)
        .with_context(|| format!("Could not create {}", path.display()))
}
