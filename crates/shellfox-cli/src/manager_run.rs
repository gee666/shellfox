//! Runs the 0.9 session, terminals, profiles, path and api commands on the
//! local host or an approved peer (src/cli/execute.ts, control.ts).
use crate::{
    human_text::{color, terminal_width, Style},
    manager::{self, Call},
    manager_args::{Listing, ManagerCommand, PathOp, SessionOp, Target, TerminalOp},
    manager_local::Local,
    manager_peer::Peer,
    node_run::confirm,
    output::{mode, print_value, CliError, Mode},
    session_view,
};
use anyhow::Result;
use serde_json::{json, Value};
use shellfox_core::{SessionId, TabId};
use shellfox_service::{
    experiment::{ExperimentRequest, ExperimentResponse},
    local::LocalClient,
};

pub struct Options {
    pub json: bool,
    pub yes: bool,
    /// A peer ID or hostname; None or "local" is this machine.
    pub host: Option<String>,
}
impl Options {
    pub fn remote(&self) -> bool {
        self.host.as_deref().is_some_and(|host| host != "local")
    }
}

pub enum Host<'a> {
    Local(Local<'a>),
    Peer(Peer<'a>),
}
impl Host<'_> {
    pub async fn call(&self, call: Call) -> Result<Value> {
        match self {
            Self::Local(local) => local.call(call).await,
            Self::Peer(peer) => peer.call(call).await,
        }
    }
}

fn call(method: &str, payload: Value) -> Result<Call> {
    manager::parse(method, payload)
}
fn not_found(message: &str) -> anyhow::Error {
    CliError::new("NOT_FOUND", message, false)
}

pub async fn run(client: &LocalClient, command: ManagerCommand, options: &Options) -> Result<()> {
    if let ManagerCommand::RemoteApi { method, payload } = command {
        if options.remote() {
            return Err(CliError::usage("remote api manages the local host."));
        }
        return print_value(&remote_api(client, &method, payload, options).await?);
    }
    let host = match &options.host {
        Some(target) if options.remote() => {
            let peer = crate::manager_peer::resolve(client, target, true).await?;
            Host::Peer(Peer::connect(client, peer).await?)
        }
        _ => Host::Local(Local(client)),
    };
    let result = execute(&host, command, options).await;
    if let Host::Peer(peer) = host {
        peer.close().await;
    }
    result
}

/// 0.9 `resolveSessionId`: an explicit ID wins; context never leaves this machine.
pub async fn session_id(host: &Host<'_>, target: Target) -> Result<SessionId> {
    let Target::Id(id) = target else {
        return current(host).await;
    };
    Ok(id)
}
async fn current(host: &Host<'_>) -> Result<SessionId> {
    let Host::Local(local) = host else {
        return Err(CliError::usage(
            "A remote session ID is required. Local terminal context is never sent to another host.",
        ));
    };
    let marker = std::env::var("SHELLFOX_TERMINAL_MARKER")
        .ok()
        .and_then(|marker| marker.parse::<TabId>().ok())
        .ok_or_else(|| {
            CliError::usage(
                "No current session. Supply a session UUID or run inside a Shellfox terminal.",
            )
        })?;
    let live = local.live(marker).await?.is_some();
    let session = local
        .snapshot()
        .await?
        .sessions
        .iter()
        .find(|s| s.tabs.iter().any(|t| t.id == marker))
        .map(|s| s.id);
    match session {
        Some(id) if live => Ok(id),
        _ => Err(CliError::usage(
            "The current terminal does not identify a live owned session. Supply a session UUID.",
        )),
    }
}

async fn snapshot(host: &Host<'_>) -> Result<Value> {
    host.call(call("getSnapshot", json!({}))?).await
}
fn find_session(snapshot: &Value, id: SessionId) -> Result<Value> {
    snapshot["sessions"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["id"] == id.to_string()))
        .cloned()
        .ok_or_else(|| not_found("Session not found."))
}
fn tabs(sessions: &[Value]) -> Value {
    Value::Array(
        sessions
            .iter()
            .flat_map(|s| s["tabs"].as_array().cloned().unwrap_or_default())
            .collect(),
    )
}

pub(crate) async fn execute(
    host: &Host<'_>,
    command: ManagerCommand,
    options: &Options,
) -> Result<()> {
    let value = match command {
        ManagerCommand::Sessions(listing, filters) => {
            return sessions(host, listing, filters).await
        }
        ManagerCommand::Session(op, target) => {
            session(host, op, session_id(host, target).await?, options).await?
        }
        ManagerCommand::Terminals(op) => terminals(host, op, options).await?,
        ManagerCommand::CreateSession {
            cwd,
            title,
            request_id,
        } => {
            let cwd = match host {
                Host::Local(_) => cwd,
                Host::Peer(_) => host
                    .call(call("resolveDirectory", json!({ "path": cwd }))?)
                    .await?["cwd"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        anyhow::anyhow!("The remote host did not resolve the folder.")
                    })?,
            };
            let mut payload = json!({ "cwd": cwd, "requestId": request_id });
            if let Some(title) = title {
                payload["title"] = json!(title);
            }
            host.call(call("createSession", payload)?).await?
        }
        ManagerCommand::Settings => snapshot(host).await?["settings"].take(),
        ManagerCommand::Profiles => host.call(call("getTerminalProfiles", json!({}))?).await?,
        ManagerCommand::Path(op) => {
            let (method, path) = match op {
                PathOp::Home => ("getHomeDirectory", None),
                PathOp::Resolve(path) => ("resolveDirectory", Some(path)),
                PathOp::Complete(path) => ("completeDirectory", Some(path)),
                PathOp::Create(path) => ("createDirectory", Some(path)),
            };
            let payload = path.map_or_else(|| json!({}), |path| json!({ "path": path }));
            host.call(call(method, payload)?).await?
        }
        ManagerCommand::Api { method, payload } => {
            let request = call(&method, payload)?;
            if request.destructive() {
                confirm(options.json, options.yes, request.name())?;
            }
            host.call(request).await?
        }
        ManagerCommand::RemoteApi { .. } => unreachable!("handled before connecting"),
    };
    print_value(&value)
}

async fn sessions(
    host: &Host<'_>,
    listing: Listing,
    mut filters: crate::manager_args::Filters,
) -> Result<()> {
    if filters.current && filters.id.is_none() {
        if matches!(host, Host::Peer(_)) {
            return Err(CliError::usage(
                "A remote session ID or --all is required; local terminal context is never forwarded.",
            ));
        }
        filters.id = Some(current(host).await?);
    }
    let payload = filters
        .id
        .map_or_else(|| json!({}), |id| json!({ "sessionId": id }));
    let stats = host.call(call("getSessionStats", payload)?).await?;
    let result = session_view::filter(&stats, &filters);
    if mode() != Mode::Human {
        return print_value(if listing == Listing::List {
            &result["sessions"]
        } else {
            &result
        });
    }
    let text = session_view::format(listing, &result, Style(color(true)), terminal_width());
    crate::output::print_text(&text)
}

async fn session(
    host: &Host<'_>,
    op: SessionOp,
    id: SessionId,
    options: &Options,
) -> Result<Value> {
    let target = json!({ "sessionId": id });
    let with = |extra: Value| {
        let mut payload = target.clone();
        if let (Some(payload), Value::Object(extra)) = (payload.as_object_mut(), extra) {
            payload.extend(extra);
        }
        payload
    };
    let request = match op {
        SessionOp::Inspect => return find_session(&snapshot(host).await?, id),
        SessionOp::Env(None) => return Ok(find_session(&snapshot(host).await?, id)?["env"].take()),
        SessionOp::Env(Some(env)) => call("setSessionEnv", with(json!({ "env": env })))?,
        SessionOp::Rename(title) => call("renameSession", with(json!({ "title": title })))?,
        SessionOp::Pin(pinned) => call("setSessionPinned", with(json!({ "pinned": pinned })))?,
        SessionOp::Settle { confirm_active } => {
            if confirm_active {
                confirm(options.json, options.yes, &format!("session settle {id}"))?;
            }
            call(
                "settleSession",
                with(json!({ "confirmActive": confirm_active })),
            )?
        }
        SessionOp::Unsettle => call("unsettleSession", target)?,
        SessionOp::ClearError => call("clearSessionError", target)?,
        SessionOp::Focus => Call::Focus(id),
        SessionOp::Refresh => Call::Refresh(id),
        SessionOp::Delete => {
            if !options.yes && mode() == Mode::Human && crate::node_run::interactive() {
                let stats = host.call(call("getSessionStats", target.clone())?).await?;
                let style = Style(color(true));
                let preview = session_view::format(Listing::List, &stats, style, terminal_width())
                    + &session_view::format(Listing::Processes, &stats, style, terminal_width());
                crate::output::print_text(&preview)?;
            }
            confirm(options.json, options.yes, &format!("session delete {id}"))?;
            call("deleteSession", target)?
        }
    };
    host.call(request).await
}

#[path = "manager_terminals.rs"]
mod terminals;
use terminals::terminals;
#[cfg(test)]
pub(crate) use terminals::write_peer;
#[path = "manager_remote_api.rs"]
mod remote_api;
use remote_api::remote_api;
