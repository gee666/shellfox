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
    path::{Path, PathBuf},
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

pub fn run(
    invocation: Invocation,
    selection: crate::args::Selection,
    watch: Option<crate::parent_watch::Watch>,
) -> Result<i32> {
    // An explicit --data-dir names a Rust host directly: no 0.9 data, no desktop cold start.
    let legacy = selection.data_dir.is_none();
    let user_data = match &selection.data_dir {
        Some(_) => invocation.user_data.clone(),
        None => Some(invocation.user_data()?),
    };
    let Invocation {
        json, yes, action, ..
    } = invocation;
    let data = match (&selection.data_dir, &user_data) {
        (Some(data), _) => data.clone(),
        (None, Some(root)) => data_dir(root),
        (None, None) => unreachable!("default user data resolved above"),
    };
    let target = || -> Result<Target> {
        let (helper, digest) = selection.helper()?;
        Ok(Target {
            data: data.clone(),
            helper,
            digest,
        })
    };
    let legacy_host = |runtime: &tokio::runtime::Runtime| {
        legacy
            .then(|| user_data.as_deref())
            .flatten()
            .map(|root| runtime.block_on(LegacyHost::connect(root)))
            .flatten()
    };
    let runtime = || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
    };
    match action {
        Action::Help => print_value(&HELP)?,
        Action::Unknown(words) if !legacy => {
            crate::output::set_mode(crate::output::Mode::Raw);
            let args = selection.args(data, &words)?;
            return runtime()?.block_on(crate::commands::run(args));
        }
        Action::Unknown(_) => {
            return Err(CliError::usage(
                "Unknown or unsupported command. Run shellfox help for the commands this version supports.",
            ))
        }
        Action::Version => print_value(&env!("SHELLFOX_VERSION"))?,
        Action::Update { check } if crate::desktop_handoff::desktop_binary().is_some() => {
            if json {
                return Err(CliError::usage("Desktop updates use api getUpdateStatus or api downloadUpdate for JSON automation."));
            }
            let desktop =
                crate::desktop_handoff::desktop_binary().context("Desktop binary missing")?;
            return crate::desktop_handoff::update(&desktop, check);
        }
        Action::Update { check } => {
            let result = runtime()?.block_on(crate::headless_update::run(check))?;
            if json {
                print_value(&result)?
            } else {
                print_value(&result.message)?
            }
        }
        Action::HostRun => {
            let runtime = runtime()?;
            if let Some(legacy) = legacy_host(&runtime) {
                runtime.block_on(legacy.close());
                return Err(CliError::new("HOST_RUNNING", "A Shellfox 0.9 host still owns this data directory. Stop it with host stop --yes, or restart it through its startup manager.", false));
            }
            drop(runtime);
            let target = target()?;
            return host_run(&target, selection, json, watch);
        }
        Action::HostStatus => {
            let (runtime, target) = (runtime()?, target()?);
            if let Ok(client) = runtime.block_on(target.connect()) {
                let _ = runtime.block_on(client.disconnect());
                print_value(&serde_json::json!({ "running": true }))?;
            } else {
                let legacy = legacy_host(&runtime).ok_or_else(unavailable)?;
                let status = runtime.block_on(legacy.status());
                runtime.block_on(legacy.close());
                print_value(&status?)?;
            }
        }
        Action::HostStop => {
            confirm(json, yes, "Stop the host and all its terminals")?;
            let (runtime, target) = (runtime()?, target()?);
            if let Ok(client) = runtime.block_on(target.connect()) {
                runtime.block_on(client.execute(ExperimentRequest::Stop {}))?;
            } else {
                let legacy = legacy_host(&runtime).ok_or_else(unavailable)?;
                runtime.block_on(legacy.stop())?;
            }
            print_value(&serde_json::json!({ "stopped": true }))?
        }
        Action::Service { install, startup } => {
            if !install {
                confirm(json, yes, "Remove host startup and stop the host")?;
            }
            let message = crate::host_service::run(
                install,
                startup,
                user_data.as_deref().context("host service needs --user-data, not --data-dir")?,
                &target()?,
            )?;
            print_value(&message)?;
        }
        Action::Host {
            words,
            confirm: what,
        } => {
            if let Some(what) = what {
                confirm(json, yes, &what)?;
            }
            let args = selection.args(data, &words)?;
            if let Some(watch) = watch {
                watch.spawn(|| std::process::exit(1));
            }
            return runtime()?.block_on(crate::commands::run(args));
        }
        Action::CreateSession { cwd, .. }
            if legacy && !json && !crate::desktop_handoff::endpoint_exists(&data) =>
        {
            // 0.9.x: with no host running, a desktop install opens the window on the new session.
            match crate::desktop_handoff::desktop_binary() {
                Some(desktop) => crate::desktop_handoff::open_session(&desktop, &cwd)?,
                None => return Err(unavailable()),
            }
        }
        action => runtime()?.block_on(connected(&target()?, action))?,
    }
    Ok(0)
}

async fn connected(target: &Target, action: Action) -> Result<()> {
    let client = target.connect().await?;
    let result = query(&client, action).await;
    let _ = client.disconnect().await;
    result
}

async fn query(client: &LocalClient, action: Action) -> Result<()> {
    match action {
        Action::CreateSession {
            cwd,
            title,
            request_id,
        } => {
            let before = snapshot(client).await?;
            let profile_id = shellfox_service::desktop_launch::folder_profile(&cwd)
                .unwrap_or_else(|| default_profile(&before));
            let input = CreateSession {
                request_id: request_id.unwrap_or_else(SessionId::new),
                title,
                cwd,
                profile_id,
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
            let old: Vec<_> = before
                .sessions
                .iter()
                .flat_map(|s| &s.tabs)
                .map(|t| t.id)
                .collect();
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

/// The Rust host this invocation addresses.
pub struct Target {
    pub data: PathBuf,
    helper: PathBuf,
    digest: [u8; 32],
}
impl Target {
    pub async fn connect(&self) -> Result<LocalClient> {
        LocalClient::connect(&self.data, &self.helper, self.digest)
            .await
            .map_err(|_| unavailable())
    }
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
fn host_run(
    target: &Target,
    selection: crate::args::Selection,
    json: bool,
    watch: Option<crate::parent_watch::Watch>,
) -> Result<i32> {
    create_private_dir(&target.data)?;
    let args = selection.args(target.data.clone(), &["host".into(), "serve".into()])?;
    let started = crate::time_text::now_iso();
    if let Some(watch) = watch {
        let target = Target {
            data: target.data.clone(),
            helper: target.helper.clone(),
            digest: target.digest,
        };
        watch.spawn(move || {
            let stop = async {
                let client = target.connect().await?;
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
