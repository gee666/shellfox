use super::*;

/// `remote api <method>`: the local host's network management, as 0.9 named it.
pub(super) async fn remote_api(
    client: &LocalClient,
    method: &str,
    payload: Value,
    options: &Options,
) -> Result<Value> {
    use shellfox_service::remote::RemoteRequest;
    let peer = || -> Result<String> {
        let object = payload.as_object().filter(|o| o.len() == 1);
        object
            .and_then(|o| o.get("peerId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| CliError::usage("Invalid remote input."))
    };
    let request = match method {
        "getState" => {
            if payload.as_object().is_none_or(|o| !o.is_empty()) {
                return Err(CliError::usage("Invalid remote input."));
            }
            RemoteRequest::State
        }
        "configure" => {
            let enabled = payload["enabled"].as_bool();
            let hostname = payload["hostname"].as_str();
            let (Some(enabled), Some(hostname), Some(2)) =
                (enabled, hostname, payload.as_object().map(|o| o.len()))
            else {
                return Err(CliError::usage("Invalid remote input."));
            };
            if !enabled {
                confirm(options.json, options.yes, "remote configure")?;
            }
            RemoteRequest::Configure {
                enabled,
                hostname: hostname.to_owned(),
            }
        }
        "login" => {
            if payload.get("authKey").is_some() {
                return Err(CliError::usage("Use network login --auth-key-stdin or --auth-key-file. Auth keys must not appear in argv."));
            }
            if payload.as_object().is_none_or(|o| !o.is_empty()) {
                return Err(CliError::usage("Invalid remote input."));
            }
            RemoteRequest::Login { auth_key: None }
        }
        "connect" => RemoteRequest::Connect { peer_id: peer()? },
        "disconnect" => RemoteRequest::Disconnect { peer_id: peer()? },
        "approve" => RemoteRequest::Approve { peer_id: peer()? },
        operation @ ("reject" | "revoke" | "forget") => {
            let peer_id = peer()?;
            confirm(options.json, options.yes, &format!("remote {operation}"))?;
            match operation {
                "reject" => RemoteRequest::Reject { peer_id },
                "revoke" => RemoteRequest::Revoke { peer_id },
                _ => RemoteRequest::Forget { peer_id },
            }
        }
        "request" => {
            let object = payload.as_object().filter(|o| {
                o.keys()
                    .all(|k| ["peerId", "method", "payload"].contains(&k.as_str()))
            });
            let (Some(peer_id), Some(method)) = (
                object.and_then(|o| o.get("peerId")).and_then(Value::as_str),
                object.and_then(|o| o.get("method")).and_then(Value::as_str),
            ) else {
                return Err(CliError::usage("Invalid remote input."));
            };
            let request = call(
                method,
                payload.get("payload").cloned().unwrap_or_else(|| json!({})),
            )?;
            if !matches!(request, Call::Remote(_)) {
                return Err(CliError::usage("Invalid remote input."));
            }
            if request.destructive() {
                confirm(options.json, options.yes, request.name())?;
            }
            let peer = crate::manager_peer::resolve(client, peer_id).await?;
            let peer = Peer::connect(client, peer).await?;
            let result = peer.call(request).await;
            peer.close().await;
            return result;
        }
        _ => return Err(CliError::usage("Unknown remote method.")),
    };
    request
        .validate()
        .map_err(|_| CliError::usage("Invalid remote input."))?;
    Ok(serde_json::to_value(client.remote_control(request).await?)?)
}
