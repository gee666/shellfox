//! Runs the 0.9.x command surface against the Rust host.
use crate::manager_args::ManagerCommand;
use crate::{
    legacy_host::LegacyHost,
    node_args::{data_dir, Action, Invocation},
    output::{print_value, CliError},
};
use anyhow::{Context, Result};
use shellfox_core::SessionId;
use shellfox_service::{experiment::ExperimentRequest, local::LocalClient, remote::RemoteRequest};
use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};

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
        json,
        yes,
        host,
        action,
        ..
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
        Action::Help(topic) => {
            let color = !json && crate::human_text::color(true);
            let text = crate::help::text(topic.as_deref(), crate::human_text::Style(color), crate::human_text::terminal_width());
            crate::output::print_text(&text)?
        }
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
        Action::CreateSession { path, .. }
            if legacy && host.is_none() && !json && !crate::desktop_handoff::endpoint_exists(&data) =>
        {
            // 0.9.x: with no host running, a desktop install opens the window on the new session.
            let cwd = crate::node_args::local_directory(path.as_deref())?;
            match crate::desktop_handoff::desktop_binary() {
                Some(desktop) => crate::desktop_handoff::open_session(&desktop, &cwd)?,
                None => return Err(unavailable()),
            }
        }
        Action::Completion(shell) => return crate::completion::run(shell.as_deref()),
        Action::Commands(words) => {
            let root = user_data.as_deref().context("Custom commands need --user-data, not --data-dir")?;
            return crate::custom_commands::run(&words, root, json);
        }
        action => {
            let options = crate::manager_run::Options { json, yes, host };
            runtime()?.block_on(connected(&target()?, action, &options))?
        }
    }
    Ok(0)
}

async fn connected(
    target: &Target,
    action: Action,
    options: &crate::manager_run::Options,
) -> Result<()> {
    let client = target.connect().await?;
    let result = query(&client, action, options).await;
    let _ = client.disconnect().await;
    result
}

async fn query(
    client: &LocalClient,
    action: Action,
    options: &crate::manager_run::Options,
) -> Result<()> {
    match action {
        Action::Manager(command) => crate::manager_run::run(client, command, options).await,
        Action::CreateSession {
            path,
            title,
            request_id,
        } => {
            let cwd = if options.remote() {
                path.unwrap_or_else(|| "~".into())
            } else {
                crate::node_args::local_directory(path.as_deref())?
            };
            crate::manager_run::run(
                client,
                ManagerCommand::CreateSession {
                    cwd,
                    title,
                    request_id: request_id.unwrap_or_else(SessionId::new),
                },
                options,
            )
            .await
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

pub(crate) fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

pub(crate) fn confirm(json: bool, yes: bool, what: &str) -> Result<()> {
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
