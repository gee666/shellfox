//! 0.9 manager methods (`api <ManagerMethod>`), the typed requests behind the
//! session, terminals, profiles and path commands. The v1 remote allowlist
//! (`RemoteCommand`) is the same method set, so one request runs on the local
//! host or, with `--host`, on an approved peer.
use crate::output::CliError;
use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use shellfox_core::{SessionId, SettingsPatch};
use shellfox_protocol::{RemoteCommand, Validate, REMOTE_METHODS};

#[derive(Debug)]
pub enum Call {
    Remote(RemoteCommand),
    Focus(SessionId),
    Refresh(SessionId),
    SaveSettings(Box<SettingsPatch>),
    GetKeepAwake,
    SetKeepAwake(bool),
}

/// 0.9 methods with no Rust equivalent. They fail with UNSUPPORTED, not VALIDATION.
const UNAVAILABLE: &[&str] = &[
    "prepareSessionRegistration",
    "retryTab",
    "setExplorerIntegration",
    "setCliIntegration",
    "listSshProfiles",
    "saveSshProfile",
    "deleteSshProfile",
    "importPuttySessions",
    "chooseDirectory",
    "chooseSshKeyFile",
    "copyText",
    "readClipboardText",
    "openSessionFolder",
    "openFileExplorer",
    "openExternal",
    "getUpdateStatus",
    "downloadUpdate",
    "installUpdate",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionInput {
    session_id: SessionId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Active {
    active: bool,
}

fn input<T: serde::de::DeserializeOwned>(method: &str, payload: Value) -> Result<T> {
    serde_json::from_value(payload).map_err(|_| CliError::usage(format!("Invalid {method} input.")))
}

pub fn parse(method: &str, payload: Value) -> Result<Call> {
    Ok(match method {
        "focusSession" => Call::Focus(input::<SessionInput>(method, payload)?.session_id),
        "refreshSessionMembership" => {
            Call::Refresh(input::<SessionInput>(method, payload)?.session_id)
        }
        "saveSettings" => Call::SaveSettings(Box::new(input(method, payload)?)),
        "getKeepAwake" => {
            input::<Empty>(method, payload)?;
            Call::GetKeepAwake
        }
        "setKeepAwake" => Call::SetKeepAwake(input::<Active>(method, payload)?.active),
        method if UNAVAILABLE.contains(&method) => {
            return Err(CliError::new(
                "UNSUPPORTED",
                format!("{method} is not available in this version of Shellfox."),
                false,
            ))
        }
        method if REMOTE_METHODS.contains(&method) => {
            let command: RemoteCommand =
                input(method, json!({ "method": method, "payload": payload }))?;
            command
                .validate()
                .map_err(|_| CliError::usage(format!("Invalid {method} input.")))?;
            Call::Remote(command)
        }
        _ => return Err(CliError::usage("Unknown ManagerMethod.")),
    })
}

impl Call {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Remote(command) => command.method_name(),
            Self::Focus(_) => "focusSession",
            Self::Refresh(_) => "refreshSessionMembership",
            Self::SaveSettings(_) => "saveSettings",
            Self::GetKeepAwake => "getKeepAwake",
            Self::SetKeepAwake(_) => "setKeepAwake",
        }
    }
    /// 0.9 asked before these, and refused them without --yes when it couldn't ask.
    pub fn destructive(&self) -> bool {
        matches!(
            self,
            Self::Remote(
                RemoteCommand::DeleteSession(_)
                    | RemoteCommand::CloseTab(_)
                    | RemoteCommand::SettleSession(shellfox_protocol::SettleSession {
                        confirm_active: true,
                        ..
                    })
            )
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn method_names_and_payloads_are_the_0_9_ones() {
        let id = "12345678-1234-4234-8234-123456789abc";
        let call = parse(
            "setSessionPinned",
            json!({ "sessionId": id, "pinned": true }),
        )
        .unwrap();
        assert_eq!(call.name(), "setSessionPinned");
        assert!(!call.destructive());
        assert!(parse("deleteSession", json!({ "sessionId": id }))
            .unwrap()
            .destructive());
        assert!(parse(
            "settleSession",
            json!({ "sessionId": id, "confirmActive": true })
        )
        .unwrap()
        .destructive());
        assert!(matches!(
            parse("focusSession", json!({ "sessionId": id })),
            Ok(Call::Focus(_))
        ));
        let code = |error: anyhow::Error| error.downcast_ref::<CliError>().unwrap().code.clone();
        assert_eq!(code(parse("nope", json!({})).unwrap_err()), "VALIDATION");
        assert_eq!(
            code(parse("setSessionPinned", json!({ "sessionId": id })).unwrap_err()),
            "VALIDATION"
        );
        assert_eq!(
            code(parse("installUpdate", json!({})).unwrap_err()),
            "UNSUPPORTED"
        );
    }
}
