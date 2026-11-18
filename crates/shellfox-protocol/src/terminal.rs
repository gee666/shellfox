use crate::{validation::*, *};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Control {
    Owned,
    Blocked,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenState {
    Open,
    Closed,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum DataTag {
    #[serde(rename = "data")]
    Data,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalData {
    #[serde(rename = "type")]
    pub kind: DataTag,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub generation: Generation,
    pub sequence: u64,
    pub data: String,
}
impl Validate for TerminalData {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.sequence)?;
        ensure(!self.data.is_empty() && self.data.len() <= 16384)
    }
}
/// ANSI recovery chunks when snapshot=true, raw PTY replay otherwise. Never Ghostty binary data.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedAttachment {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub generation: Generation,
    pub control_epoch: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub chunks: Vec<TerminalData>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub snapshot: Option<bool>,
    pub state: ScreenState,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub exit_code: Option<i64>,
    pub cols: u16,
    pub rows: u16,
    pub lifetime: TerminalLifetime,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    pub settled_at: Option<Option<String>>,
}
impl Validate for OwnedAttachment {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.control_epoch)?;
        epoch(self.first_sequence)?;
        sequence(self.last_sequence)?;
        ensure(self.first_sequence <= self.last_sequence + 1 && self.chunks.len() <= 8192)?;
        ensure((2..=500).contains(&self.cols) && (2..=500).contains(&self.rows))?;
        ensure(matches!(self.lifetime, TerminalLifetime::AppOwned))?;
        if let Some(code) = self.exit_code {
            integer(code)?;
        }
        if let Some(value) = &self.settled_at {
            optional_time(value)?;
        }
        let mut previous = 0;
        let mut bytes = 0;
        for chunk in &self.chunks {
            chunk.validate()?;
            ensure(
                chunk.tab_id == self.tab_id
                    && chunk.generation == self.generation
                    && chunk.sequence >= self.first_sequence
                    && chunk.sequence <= self.last_sequence
                    && chunk.sequence > previous,
            )?;
            previous = chunk.sequence;
            bytes += chunk.data.len();
        }
        ensure(bytes <= 262144)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "control", rename_all = "lowercase", deny_unknown_fields)]
pub enum TerminalAttachment {
    Owned(OwnedAttachment),
    #[serde(rename_all = "camelCase")]
    Blocked {
        #[serde(deserialize_with = "crate::ids::wire_id")]
        tab_id: TabId,
        #[serde(deserialize_with = "crate::ids::wire_id")]
        session_id: SessionId,
        #[serde(deserialize_with = "crate::ids::wire_id")]
        generation: Generation,
        control_epoch: u64,
    },
}
impl Validate for TerminalAttachment {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Owned(value) => value.validate(),
            Self::Blocked { control_epoch, .. } => epoch(*control_epoch),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum TerminalEvent {
    #[serde(rename_all = "camelCase")]
    Settlement {
        #[serde(deserialize_with = "crate::ids::wire_id")]
        tab_id: TabId,
        #[serde(deserialize_with = "crate::ids::wire_id")]
        generation: Generation,
        #[serde(deserialize_with = "crate::validation::nullable")]
        settled_at: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Activity { tab_id: TabId, busy: bool },
    #[serde(rename_all = "camelCase")]
    Data {
        #[serde(deserialize_with = "crate::ids::wire_id")]
        tab_id: TabId,
        #[serde(deserialize_with = "crate::ids::wire_id")]
        generation: Generation,
        control_epoch: u64,
        sequence: u64,
        data: String,
    },
    #[serde(rename_all = "camelCase")]
    Exit {
        #[serde(deserialize_with = "crate::ids::wire_id")]
        tab_id: TabId,
        #[serde(deserialize_with = "crate::ids::wire_id")]
        generation: Generation,
        control_epoch: u64,
        #[serde(deserialize_with = "crate::validation::nullable")]
        exit_code: Option<i64>,
        #[serde(deserialize_with = "crate::validation::nullable")]
        signal: Option<i64>,
        last_sequence: u64,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        #[serde(deserialize_with = "crate::ids::wire_id")]
        tab_id: TabId,
        #[serde(deserialize_with = "crate::ids::wire_id")]
        generation: Generation,
        control_epoch: u64,
        error: AppError,
    },
    #[serde(rename_all = "camelCase")]
    Control {
        #[serde(deserialize_with = "crate::ids::wire_id")]
        tab_id: TabId,
        #[serde(deserialize_with = "crate::ids::wire_id")]
        generation: Generation,
        control: Control,
        control_epoch: u64,
    },
}
impl Validate for TerminalEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Settlement { settled_at, .. } => optional_time(settled_at),
            Self::Activity { .. } => Ok(()),
            Self::Data {
                control_epoch,
                sequence,
                data,
                ..
            } => {
                epoch(*control_epoch)?;
                epoch(*sequence)?;
                ensure(!data.is_empty() && data.len() <= 16384)
            }
            Self::Exit {
                control_epoch,
                last_sequence,
                exit_code,
                signal,
                ..
            } => {
                epoch(*control_epoch)?;
                if let Some(code) = exit_code {
                    integer(*code)?;
                }
                if let Some(code) = signal {
                    integer(*code)?;
                }
                sequence(*last_sequence)
            }
            Self::Error {
                control_epoch,
                error,
                ..
            } => {
                epoch(*control_epoch)?;
                error.validate()
            }
            Self::Control { control_epoch, .. } => epoch(*control_epoch),
        }
    }
}
/// v1 implies ANSI replay/recovery and exclusive-v1 fencing. No binary format is negotiated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryFormat {
    AnsiV1,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalProfile {
    pub id: String,
    pub label: String,
    pub environment: ProfileEnvironment,
    pub executable: String,
    pub args: Vec<String>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub distro: Option<String>,
    pub available: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    pub unavailable_reason: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub can_terminate_descendants: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub elevation: Option<Elevation>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileEnvironment {
    Local,
    Wsl,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Elevation {
    Administrator,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalProfiles {
    pub profiles: Vec<TerminalProfile>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub default_profile_id: Option<String>,
    pub lifetime: TerminalLifetime,
    pub shell_survival: bool,
}
impl Validate for TerminalProfiles {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(
            self.profiles.len() <= 100
                && matches!(self.lifetime, TerminalLifetime::AppOwned)
                && !self.shell_survival,
        )?;
        if let Some(id) = &self.default_profile_id {
            profile(id)?;
        }
        for p in &self.profiles {
            profile(&p.id)?;
            title(&p.label)?;
            cwd(&p.executable)?;
            ensure(p.args.len() <= 30)?;
            for arg in &p.args {
                ensure(arg.encode_utf16().count() <= 4096)?;
            }
            if let Some(distro) = &p.distro {
                text(distro, 200)?;
                ensure(!distro.is_empty())?;
            }
            if let Some(Some(reason)) = &p.unavailable_reason {
                text(reason, 1000)?;
            }
        }
        Ok(())
    }
}
