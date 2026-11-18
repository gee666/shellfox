use crate::remote_args::NetworkCommand;
use anyhow::{Result, bail};
use shellfox_service::{
    local::LocalClient,
    remote::{AuthKey, RemoteRequest},
};
use std::io::{IsTerminal, Read};

pub async fn run(client: &LocalClient, command: NetworkCommand) -> Result<()> {
    let changing = !matches!(&command, NetworkCommand::Request(RemoteRequest::State));
    let request = match command {
        NetworkCommand::Request(request) => request,
        NetworkCommand::Configure { enabled, hostname } => {
            let hostname = match hostname {
                Some(value) => value,
                None => client.remote_control(RemoteRequest::State).await?.hostname,
            };
            RemoteRequest::Configure { enabled, hostname }
        }
        NetworkCommand::LoginFromStdin => {
            let input = std::io::stdin();
            if input.is_terminal() {
                bail!(
                    "--auth-key-stdin requires redirected standard input to avoid terminal echo."
                );
            }
            RemoteRequest::Login {
                auth_key: Some(read_auth_key(input.lock())?),
            }
        }
    };
    // Same authenticated local epoch and original actor; no network/PAIR client.
    let state = client.remote_control(request).await?;
    crate::commands::print_json(&state)?;
    if changing && (state.error.is_some() || state.cleanup_retained) {
        bail!("Local network operator is not confirmed; inspect the returned state.");
    }
    Ok(())
}

fn read_auth_key(input: impl Read) -> Result<AuthKey> {
    let mut bytes = Vec::new();
    input
        .take(4099)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Unable to read login key from standard input."))?;
    if bytes.len() > 4098 {
        bail!("Invalid authentication key input.");
    }
    let mut value = String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid authentication key input."))?;
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    AuthKey::new(value).map_err(|_| anyhow::anyhow!("Invalid authentication key input."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_one_line_key_never_enters_debug_or_errors() {
        let secret = "tskey-auth-secret-PRIVATE";
        for suffix in ["", "\n", "\r\n"] {
            let key = read_auth_key(std::io::Cursor::new(format!("{secret}{suffix}"))).unwrap();
            assert_eq!(format!("{key:?}"), "AuthKey([redacted])");
        }
        for bytes in [
            vec![],
            vec![b'x'; 4100],
            vec![0xff],
            format!("{secret}\nextra").into_bytes(),
        ] {
            let error = read_auth_key(std::io::Cursor::new(bytes)).unwrap_err();
            assert!(!format!("{error:#}").contains(secret));
        }
    }
}
