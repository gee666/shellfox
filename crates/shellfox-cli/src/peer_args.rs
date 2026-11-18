//! Remote-consumer syntax. Kept separate from Eva's local/serve/bootstrap parser.
use anyhow::{Result, bail, ensure};
use shellfox_protocol::{AuthenticatedPeerId, Validate, manager_v2 as m};
use std::ffi::OsString;

#[derive(Clone, Debug)]
pub(crate) enum PeerCommand {
    Sessions,
    Tabs { session: Option<m::Id> },
    Create { cwd: String, title: Option<String> },
    Attach { tab: m::Id, takeover: bool },
}
#[derive(Clone, Debug)]
pub(crate) struct PeerArgs {
    pub peer: AuthenticatedPeerId,
    pub command: PeerCommand,
}
pub(crate) const HELP: &str = "peer PEER sessions | tabs [SESSION] | create --cwd HOST_ABSOLUTE_PATH [--title TITLE] | attach TAB [--takeover]";
pub(crate) fn parse_peer(values: &[OsString]) -> Result<Option<PeerArgs>> {
    if values.first().and_then(|s| s.to_str()) != Some("peer") {
        return Ok(None);
    }
    let values: Vec<_> = values
        .iter()
        .map(|s| {
            s.to_str()
                .ok_or_else(|| anyhow::anyhow!("Peer arguments must be UTF-8."))
        })
        .collect::<Result<_>>()?;
    ensure!(values.len() >= 3, "{HELP}");
    let peer = AuthenticatedPeerId::new(values[1])?;
    let command = match values[2] {
        "sessions" if values.len() == 3 => PeerCommand::Sessions,
        "tabs" if values.len() <= 4 => PeerCommand::Tabs {
            session: values.get(3).map(|value| value.parse()).transpose()?,
        },
        "attach" if values.len() == 4 || values.len() == 5 => {
            ensure!(values.len() == 4 || values[4] == "--takeover", "{HELP}");
            PeerCommand::Attach {
                tab: values[3].parse()?,
                takeover: values.len() == 5,
            }
        }
        "create" => {
            let mut cwd = None;
            let mut title = None;
            let mut index = 3;
            while index < values.len() {
                let flag = values[index];
                index += 1;
                let value = values
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("Missing value for {flag}"))?;
                index += 1;
                match flag {
                    "--cwd" if cwd.is_none() => cwd = Some((*value).to_owned()),
                    "--title" if title.is_none() => title = Some((*value).to_owned()),
                    _ => bail!("Unknown or duplicate peer create option: {flag}"),
                }
            }
            let cwd =
                cwd.ok_or_else(|| anyhow::anyhow!("Peer create needs --cwd HOST_ABSOLUTE_PATH."))?;
            shellfox_protocol::CreateSession {
                title: title.clone(),
                cwd: cwd.clone(),
                request_id: shellfox_protocol::SessionId::new(),
            }
            .validate()?;
            PeerCommand::Create { cwd, title }
        }
        _ => bail!("{HELP}"),
    };
    Ok(Some(PeerArgs { peer, command }))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn remote_prefix_does_not_consume_local_or_serve_options() {
        assert!(parse_peer(&args(&["serve"])).unwrap().is_none());
        assert!(parse_peer(&args(&["sessions"])).unwrap().is_none());
        let parsed = parse_peer(&args(&["peer", "n-work", "sessions"]))
            .unwrap()
            .unwrap();
        assert_eq!(parsed.peer.as_str(), "n-work");
        assert!(matches!(parsed.command, PeerCommand::Sessions));
    }
    #[test]
    fn concrete_command_fields_are_preserved_and_takeover_is_explicit() {
        let id = "00000000-0000-4000-8000-000000000001";
        let Some(PeerArgs {
            command: PeerCommand::Tabs { session },
            ..
        }) = parse_peer(&args(&["peer", "n-work", "tabs", id])).unwrap()
        else {
            panic!("tabs command");
        };
        assert_eq!(session.unwrap().to_string(), id);
        let Some(PeerArgs {
            command: PeerCommand::Attach { tab, takeover },
            ..
        }) = parse_peer(&args(&["peer", "n-work", "attach", id, "--takeover"])).unwrap()
        else {
            panic!("attach command");
        };
        assert_eq!(tab.to_string(), id);
        assert!(takeover);
        let Some(PeerArgs {
            command: PeerCommand::Attach { takeover, .. },
            ..
        }) = parse_peer(&args(&["peer", "n-work", "attach", id])).unwrap()
        else {
            panic!("attach command");
        };
        assert!(!takeover);
        let Some(PeerArgs {
            command: PeerCommand::Create { cwd, title },
            ..
        }) = parse_peer(&args(&[
            "peer",
            "n-work",
            "create",
            "--cwd",
            "/work",
            "--title",
            "actual title",
        ]))
        .unwrap()
        else {
            panic!("create command");
        };
        assert_eq!(cwd, "/work");
        assert_eq!(title.as_deref(), Some("actual title"));
    }
    #[test]
    fn peer_paths_are_host_validated_not_local_os_canonicalized() {
        assert!(parse_peer(&args(&["peer", "n-home", "create", "--cwd", "C:\\work"])).is_ok());
        assert!(
            parse_peer(&args(&[
                "peer",
                "n-work",
                "create",
                "--cwd",
                "/home/user/work"
            ]))
            .is_ok()
        );
        assert!(
            parse_peer(&args(&[
                "peer",
                "n-work",
                "create",
                "--cwd",
                "https://bad.invalid"
            ]))
            .is_err()
        );
        assert!(
            parse_peer(&args(&[
                "peer", "n-work", "create", "--cwd", "/a", "--cwd", "/b"
            ]))
            .is_err()
        );
    }
}
