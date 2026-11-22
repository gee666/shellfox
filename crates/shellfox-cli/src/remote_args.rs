use anyhow::{bail, Context, Result};
use shellfox_service::remote::RemoteRequest;

#[derive(Debug)]
pub enum NetworkCommand {
    Request(RemoteRequest),
    Configure {
        enabled: bool,
        hostname: Option<String>,
    },
    LoginFromStdin,
}

/// Closed local operator grammar. Never accepts a login secret in argv.
pub fn parse(words: &[&str]) -> Result<Option<NetworkCommand>> {
    if !words.starts_with(&["host", "network"]) {
        return Ok(None);
    }
    let command = match &words[2..] {
        ["status"] => NetworkCommand::Request(RemoteRequest::State),
        ["on"] => NetworkCommand::Configure {
            enabled: true,
            hostname: None,
        },
        ["off"] => NetworkCommand::Configure {
            enabled: false,
            hostname: None,
        },
        ["on", "--hostname", hostname] | ["off", "--hostname", hostname] => {
            RemoteRequest::Configure {
                enabled: true,
                hostname: (*hostname).into(),
            }
            .validate()
            .map_err(|error| anyhow::anyhow!(error.message))?;
            NetworkCommand::Configure {
                enabled: words[2] == "on",
                hostname: Some((*hostname).into()),
            }
        }
        ["login"] => NetworkCommand::Request(RemoteRequest::Login { auth_key: None }),
        ["login", "--auth-key-stdin"] => NetworkCommand::LoginFromStdin,
        [operation @ ("cancel" | "continue"), attempt] => {
            let attempt_id = attempt
                .parse()
                .context("Expected a non-nil login/connection attempt UUID.")?;
            NetworkCommand::Request(if *operation == "cancel" {
                RemoteRequest::Cancel { attempt_id }
            } else {
                RemoteRequest::Continue { attempt_id }
            })
        }
        ["peer", peer, operation] => {
            let peer_id = (*peer).to_owned();
            NetworkCommand::Request(match *operation {
                "connect" => RemoteRequest::Connect { peer_id },
                "disconnect" => RemoteRequest::Disconnect { peer_id },
                "reconnect" => RemoteRequest::Reconnect { peer_id },
                "approve" => RemoteRequest::Approve { peer_id },
                "reject" => RemoteRequest::Reject { peer_id },
                "revoke" => RemoteRequest::Revoke { peer_id },
                "forget" => RemoteRequest::Forget { peer_id },
                _ => bail!("Unsupported local network peer operation."),
            })
        }
        _ => bail!(
            "Expected a closed host network operator. Login keys are accepted only by --auth-key-stdin."
        ),
    };
    if let NetworkCommand::Request(request) = &command {
        request
            .validate()
            .map_err(|error| anyhow::anyhow!(error.message))?;
    }
    Ok(Some(command))
}

#[cfg(test)]
#[path = "remote_args_tests.rs"]
mod tests;
