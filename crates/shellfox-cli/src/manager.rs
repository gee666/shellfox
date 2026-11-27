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
    /// Read-only output read; local hosts only.
    ReadTerminal(shellfox_protocol::ReadTerminal),
    /// 0.9 kept these always on: the call only installs or repairs.
    EnsureIntegration(Integration),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integration {
    Cli,
    Explorer,
}
impl Integration {
    pub fn method(self) -> &'static str {
        match self {
            Self::Cli => "setCliIntegration",
            Self::Explorer => "setExplorerIntegration",
        }
    }
    /// The desktop's --ensure-integration argument.
    pub fn argument(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Explorer => "explorer",
        }
    }
}

/// 0.9 methods with no Rust equivalent. They fail with UNSUPPORTED, not VALIDATION.
const UNAVAILABLE: &[&str] = &[
    "prepareSessionRegistration",
    "retryTab",
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
struct Installed {
    installed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Active {
    active: bool,
}

fn input<T: serde::de::DeserializeOwned>(method: &str, payload: Value) -> Result<T> {
    serde_json::from_value(payload)
        .map_err(|error| CliError::usage(format!("Invalid {method} input: {}", issue(&error))))
}
/// serde's reason, like 0.9's "<path> <issue>", without the rejected value:
/// payloads carry env values and file text.
fn issue(error: &serde_json::Error) -> String {
    let message = error.to_string();
    for kind in ["invalid type", "invalid value", "unknown variant"] {
        if message.starts_with(kind) {
            return match message.rsplit_once(", expected ") {
                Some((_, expected)) => format!("{kind}, expected {expected}"),
                None => kind.into(),
            };
        }
    }
    message
}
/// 0.9 settings carried `pythonPath`, and `settings read` still prints it, so
/// a read-modify-write round trip drops it here.
fn settings_patch(mut payload: Value) -> Value {
    if let Some(fields) = payload.as_object_mut() {
        if fields
            .get("pythonPath")
            .is_some_and(|value| value.is_null() || value.is_string())
        {
            fields.remove("pythonPath");
        }
    }
    payload
}

pub fn parse(method: &str, payload: Value) -> Result<Call> {
    Ok(match method {
        "focusSession" => Call::Focus(input::<SessionInput>(method, payload)?.session_id),
        "refreshSessionMembership" => {
            Call::Refresh(input::<SessionInput>(method, payload)?.session_id)
        }
        "saveSettings" => Call::SaveSettings(Box::new(input(method, settings_patch(payload))?)),
        "getKeepAwake" => {
            input::<Empty>(method, payload)?;
            Call::GetKeepAwake
        }
        "setKeepAwake" => Call::SetKeepAwake(input::<Active>(method, payload)?.active),
        "readTerminal" => {
            let read: shellfox_protocol::ReadTerminal = input(method, payload)?;
            read.validate()
                .map_err(|_| CliError::usage(format!("Invalid {method} input.")))?;
            Call::ReadTerminal(read)
        }
        "setCliIntegration" | "setExplorerIntegration" => {
            let (kind, always) = if method == "setCliIntegration" {
                (Integration::Cli, "CLI integration is always enabled.")
            } else {
                (
                    Integration::Explorer,
                    "File-manager integration is always enabled.",
                )
            };
            if !input::<Installed>(method, payload)?.installed {
                return Err(CliError::usage(always));
            }
            Call::EnsureIntegration(kind)
        }
        method if UNAVAILABLE.contains(&method) => {
            return Err(CliError::coded(
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
            Self::ReadTerminal(_) => "readTerminal",
            Self::EnsureIntegration(kind) => kind.method(),
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
    #[test]
    fn integrations_can_only_be_ensured() {
        let call = parse("setExplorerIntegration", json!({ "installed": true })).unwrap();
        assert!(matches!(
            call,
            Call::EnsureIntegration(Integration::Explorer)
        ));
        assert_eq!(call.name(), "setExplorerIntegration");
        assert!(matches!(
            parse("setCliIntegration", json!({ "installed": true })),
            Ok(Call::EnsureIntegration(Integration::Cli))
        ));
        let error = |method, payload| {
            let error = parse(method, payload).unwrap_err();
            let error = error.downcast_ref::<CliError>().unwrap();
            (error.code.to_string(), error.message.clone())
        };
        assert_eq!(
            error("setCliIntegration", json!({ "installed": false })),
            (
                "VALIDATION".into(),
                "CLI integration is always enabled.".into()
            )
        );
        assert_eq!(
            error("setExplorerIntegration", json!({ "installed": false })).1,
            "File-manager integration is always enabled."
        );
        assert_eq!(error("setCliIntegration", json!({})).0, "VALIDATION");
    }
    #[test]
    fn input_errors_name_the_issue_but_not_the_value() {
        let message = |method, payload| {
            let error = parse(method, payload).unwrap_err();
            error.downcast_ref::<CliError>().unwrap().message.clone()
        };
        let id = "12345678-1234-4234-8234-123456789abc";
        let typed = message("setSessionEnv", json!({ "sessionId": id, "env": "s3cret" }));
        assert!(typed.starts_with("Invalid setSessionEnv input: invalid type, expected"));
        assert!(!typed.contains("s3cret"));
        assert!(message("focusSession", json!({})).contains("missing field `sessionId`"));
    }
    #[test]
    fn saved_settings_drop_the_retired_python_path() {
        let Call::SaveSettings(patch) = parse(
            "saveSettings",
            json!({ "pythonPath": null, "historyPageSize": 30 }),
        )
        .unwrap() else {
            panic!("saveSettings");
        };
        assert_eq!(patch.history_page_size, Some(30));
        assert!(parse("saveSettings", json!({ "pythonPath": "/usr/bin/python3" })).is_ok());
        assert!(parse("saveSettings", json!({ "pythonPath": 1 })).is_err());
    }
}
