use anyhow::{ensure, Result};
use serde::Deserialize;

const FRAME_BOUND: usize = 2048;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capability {
    version: u8,
    platform: String,
    arch: String,
    available: bool,
    #[serde(deserialize_with = "required_nullable")]
    reason: Option<String>,
    owned_session: bool,
    guardians: bool,
    termination: bool,
}
impl Capability {
    pub fn parse(bytes: &[u8], expected_arch: &str) -> Result<Self> {
        ensure!(bytes.len() <= FRAME_BOUND, "capability exceeds frame bound");
        ensure!(
            matches!(expected_arch, "x64" | "arm64"),
            "unsupported helper architecture"
        );
        let value: Self = serde_json::from_slice(bytes)?;
        ensure!(
            value.version == 1 && value.platform == "darwin" && value.arch == expected_arch,
            "incompatible Darwin supervisor"
        );
        ensure!(
            value.owned_session && value.guardians && value.termination,
            "missing ownership guarantees"
        );
        validate_reason(value.reason.as_deref())?;
        ensure!(
            value.available && value.reason.is_none(),
            "native supervisor preflight refused"
        );
        Ok(value)
    }
}

#[derive(Clone, Copy)]
pub enum ControlCommand {
    Status,
    Close,
}
impl ControlCommand {
    pub fn frame(self, token: &str) -> Result<String> {
        ensure!(
            token.len() == 64
                && token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid supervisor token"
        );
        let command = match self {
            Self::Status => "STATUS",
            Self::Close => "CLOSE",
        };
        Ok(format!("1\t{token}\t{command}\n"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlIdentity {
    pub supervisor_pid: u32,
    pub shell_pid: u32,
    pub shell_birth: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupervisorStatus {
    version: u8,
    pub ok: bool,
    pub state: String,
    pub supervisor_pid: u32,
    pub shell_pid: u32,
    pub shell_birth: String,
    pub shell_alive: bool,
    #[serde(deserialize_with = "required_nullable")]
    pub exit_code: Option<i32>,
    #[serde(deserialize_with = "required_nullable")]
    pub reason: Option<String>,
}
impl SupervisorStatus {
    /// Exactly one UTF-8 JSON line. No trailing bytes or extra control frames.
    pub fn parse(
        frame: &[u8],
        expected_pid: u32,
        pinned: Option<&ControlIdentity>,
    ) -> Result<Self> {
        ensure!(
            frame.len() <= FRAME_BOUND
                && frame.last() == Some(&b'\n')
                && !frame[..frame.len() - 1].contains(&b'\n'),
            "invalid control frame"
        );
        let value: Self = serde_json::from_slice(&frame[..frame.len() - 1])?;
        ensure!(
            value.version == 1
                && valid_pid(value.supervisor_pid)
                && value.supervisor_pid == expected_pid,
            "supervisor identity changed"
        );
        ensure!(
            value.shell_pid <= i32::MAX as u32 && value.shell_birth.len() <= 80,
            "invalid shell identity"
        );
        ensure!(
            value.shell_birth.is_empty() || valid_birth(&value.shell_birth),
            "invalid shell birth"
        );
        ensure!(
            matches!(value.state.as_str(), "open" | "closing" | "closed"),
            "invalid supervisor state"
        );
        ensure!(
            value.state != "closed" || !value.shell_alive,
            "closed supervisor still has live shell"
        );
        validate_reason(value.reason.as_deref())?;
        if let Some(identity) = pinned {
            ensure!(
                value.supervisor_pid == identity.supervisor_pid
                    && value.shell_pid == identity.shell_pid
                    && value.shell_birth == identity.shell_birth,
                "shell identity changed"
            );
        }
        Ok(value)
    }

    pub fn ready_identity(&self) -> Result<ControlIdentity> {
        ensure!(
            self.ok
                && self.state == "open"
                && self.shell_alive
                && valid_pid(self.shell_pid)
                && self.shell_pid != self.supervisor_pid
                && valid_birth(&self.shell_birth)
                && self.exit_code.is_none()
                && self.reason.is_none(),
            "shell startup was not authenticated"
        );
        Ok(ControlIdentity {
            supervisor_pid: self.supervisor_pid,
            shell_pid: self.shell_pid,
            shell_birth: self.shell_birth.clone(),
        })
    }

    pub fn confirm_closed(&self) -> Result<()> {
        ensure!(
            self.ok && self.state == "closed" && !self.shell_alive && self.reason.is_none(),
            "owned Darwin cleanup not confirmed; retain control for retry"
        );
        Ok(())
    }
}
fn required_nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
fn valid_pid(pid: u32) -> bool {
    pid > 0 && pid <= i32::MAX as u32
}
fn valid_birth(birth: &str) -> bool {
    let Some(rest) = birth.strip_prefix("darwin:") else {
        return false;
    };
    let Some((sec, micros)) = rest.split_once(':') else {
        return false;
    };
    !sec.is_empty()
        && sec.bytes().all(|b| b.is_ascii_digit())
        && sec.parse::<u64>().ok().is_some_and(|s| s > 0)
        && micros.len() == 6
        && micros.bytes().all(|b| b.is_ascii_digit())
}
fn validate_reason(reason: Option<&str>) -> Result<()> {
    if let Some(reason) = reason {
        ensure!(
            reason.len() <= 1000 && !reason.chars().any(char::is_control),
            "invalid supervisor reason"
        );
    }
    Ok(())
}
