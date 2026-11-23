//! `terminals` commands: metadata calls, and one-shot writes that never start a shell.
use super::*;

pub(super) async fn terminals(host: &Host<'_>, op: TerminalOp, options: &Options) -> Result<Value> {
    Ok(match op {
        TerminalOp::List(None) => tabs(
            snapshot(host).await?["sessions"]
                .as_array()
                .map_or(&[][..], Vec::as_slice),
        ),
        TerminalOp::List(Some(target)) => {
            let explicit = matches!(target, Target::Id(_));
            let id = session_id(host, target).await?;
            match find_session(&snapshot(host).await?, id) {
                Ok(session) => session["tabs"].clone(),
                Err(_) if !explicit => json!([]),
                Err(error) => return Err(error),
            }
        }
        TerminalOp::Create {
            session,
            title,
            profile,
            cwd,
        } => {
            let id = session_id(host, session).await?;
            let mut payload = json!({ "sessionId": id });
            if let Some(title) = title {
                payload["title"] = json!(title);
            }
            if let Some(profile) = profile {
                payload["profileId"] = json!(profile);
            }
            if let Some(path) = cwd {
                let directory = host
                    .call(call("resolveDirectory", json!({ "path": path }))?)
                    .await?;
                payload["cwd"] = directory["cwd"].clone();
            }
            host.call(call("addTab", payload)?).await?
        }
        TerminalOp::Rename {
            session,
            tab,
            title,
        } => {
            let id = session_id(host, session).await?;
            host.call(call(
                "renameTab",
                json!({ "sessionId": id, "tabId": tab, "title": title }),
            )?)
            .await?
        }
        TerminalOp::Reorder { session, tabs } => {
            let id = session_id(host, session).await?;
            host.call(call(
                "reorderTabs",
                json!({ "sessionId": id, "tabIds": tabs }),
            )?)
            .await?
        }
        TerminalOp::Close(tab) => {
            confirm(options.json, options.yes, &format!("Close terminal {tab}"))?;
            let generation = generation(host, tab).await?;
            host.call(call(
                "closeTab",
                json!({ "tabId": tab, "generation": generation }),
            )?)
            .await?
        }
        TerminalOp::Write { tab, data, take } => {
            let data = match data {
                Some(data) => data,
                None => crate::node_args::read_stdin(65536)?,
            };
            if data.is_empty() || data.len() > 65536 || data.contains('\0') {
                return Err(CliError::usage(
                    "Terminal input must be nonempty and fit within 64 KiB.",
                ));
            }
            match host {
                Host::Local(local) => write_local(local, tab, data, take).await?,
                Host::Peer(_) => {
                    let identity =
                        json!({ "tabId": tab, "generation": generation(host, tab).await? });
                    write_peer(host, identity, data, take).await?
                }
            }
        }
    })
}

/// The open terminal's generation; 0.9 never started a shell to close or write to one.
async fn generation(host: &Host<'_>, tab: TabId) -> Result<Value> {
    let missing = || not_found("Open embedded terminal not found.");
    match host {
        Host::Local(local) => Ok(json!(local.live(tab).await?.ok_or_else(missing)?)),
        Host::Peer(_) => {
            let snapshot = snapshot(host).await?;
            let sessions = snapshot["sessions"]
                .as_array()
                .map_or(&[][..], Vec::as_slice);
            let all = tabs(sessions);
            let found = all
                .as_array()
                .into_iter()
                .flatten()
                .find(|t| t["id"] == tab.to_string());
            found
                .map(|t| t["generation"].clone())
                .filter(|g| g.is_string())
                .ok_or_else(missing)
        }
    }
}

fn control_required() -> anyhow::Error {
    CliError::new(
        "TERMINAL_CONTROL_REQUIRED",
        "Another client is controlling this terminal. Use --take to switch control explicitly.",
        false,
    )
}
fn uncertain() -> anyhow::Error {
    CliError::new(
        "TARGET_LOST",
        "Terminal input delivery is uncertain; input was not retried. Inspect the terminal before sending it again.",
        false,
    )
}

/// One write, never retried. Joining is free; taking control needs --take.
async fn write_local(local: &Local<'_>, tab: TabId, data: String, take: bool) -> Result<Value> {
    generation(&Host::Local(Local(local.0)), tab).await?;
    let attach = |takeover| local.0.execute(LocalApiRequest::Attach { tab, takeover });
    let attachment = match attach(false).await {
        Ok(LocalApiResponse::Attachment(attachment)) => attachment,
        // Another client holds it. Take over only while the same shell still runs.
        Err(_) if take && local.live(tab).await?.is_some() => match attach(true).await? {
            LocalApiResponse::Attachment(attachment) => attachment,
            _ => return Err(control_required()),
        },
        Err(_) if local.live(tab).await?.is_none() => {
            return Err(CliError::new(
                "NOT_FOUND",
                "Open embedded terminal not found.",
                false,
            ))
        }
        _ => return Err(control_required()),
    };
    let input = LocalApiRequest::Input {
        tab,
        attachment,
        bytes: data.into_bytes(),
    };
    let written = local.0.execute(input).await;
    // Give control back at once; the shell keeps running.
    let _ = local.0.execute(LocalApiRequest::Detach {}).await;
    match written {
        Ok(LocalApiResponse::Done {}) => Ok(json!({ "written": true })),
        _ => Err(uncertain()),
    }
}

pub(crate) async fn write_peer(
    host: &Host<'_>,
    identity: Value,
    data: String,
    take: bool,
) -> Result<Value> {
    let mut attachment = host.call(call("attachTerminal", identity.clone())?).await?;
    let result = async {
        if attachment["control"] == "blocked" && take {
            host.call(call("takeTerminalControl", identity.clone())?)
                .await?;
            attachment = host.call(call("attachTerminal", identity.clone())?).await?;
        }
        if attachment["control"] != "owned" {
            return Err(control_required());
        }
        let mut payload = identity.clone();
        payload["controlEpoch"] = attachment["controlEpoch"].clone();
        payload["data"] = json!(data);
        if let Some(settled) = attachment.get("settledAt") {
            payload["settledAt"] = settled.clone();
        }
        match host.call(call("writeTerminal", payload)?).await {
            Ok(value) => Ok(value),
            Err(error)
                if error
                    .downcast_ref::<CliError>()
                    .is_some_and(|e| e.code != "TARGET_LOST") =>
            {
                Err(error)
            }
            Err(_) => Err(uncertain()),
        }
    }
    .await;
    let _ = host.call(call("detachTerminal", identity)?).await;
    result
}
