use crate::{validation::*, AdapterId, EnvVar, ShellId, Validate, ValidationError};
use serde::{Deserialize, Serialize};

/// Presence-preserving v2 leaf. No migration defaults or inferred known absence.
/// None is omitted; Some(None) is an explicit, proven nullable field.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub version: u8,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_nullable"
    )]
    pub python_path: Option<Option<String>>,
    pub accent_color: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub background_color: Option<String>,
    pub adapter_id: AdapterId,
    pub shell_id: ShellId,
    #[serde(deserialize_with = "nullable")]
    pub shell_executable: Option<String>,
    pub process_rules: Vec<ProcessRule>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub history_page_size: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub session_settle_hours: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub session_delete_hours: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub auto_update: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_nullable"
    )]
    pub terminal_profile_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub terminal_env: Option<Vec<EnvVar>>,
}
impl Validate for Settings {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.version == 1)?;
        color(&self.accent_color)?;
        if let Some(value) = &self.background_color {
            color(value)?;
        }
        if let Some(Some(value)) = &self.python_path {
            cwd(value)?;
        }
        if let Some(value) = &self.shell_executable {
            cwd(value)?;
        }
        if let Some(value) = self.history_page_size {
            ensure((1..=100).contains(&value))?;
        }
        for value in [self.session_settle_hours, self.session_delete_hours]
            .into_iter()
            .flatten()
        {
            ensure(value.is_finite() && (1.0..=87600.0).contains(&value))?;
        }
        if let Some(Some(value)) = &self.terminal_profile_id {
            profile(value)?;
        }
        unique(
            &self
                .process_rules
                .iter()
                .map(|value| &value.id)
                .collect::<Vec<_>>(),
            100,
        )?;
        for rule in &self.process_rules {
            rule.validate()?;
        }
        if let Some(value) = &self.terminal_env {
            env(value, false)?;
        }
        Ok(())
    }
}

/// Same rule constraints as v1, with strict optional non-null processNames.
/// This does not deserialize through a defaulting legacy leaf.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessRule {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub executable_basenames: Vec<String>,
    pub executable_paths: Vec<String>,
    pub script_path_suffixes: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub process_names: Option<Vec<String>>,
}
impl Validate for ProcessRule {
    fn validate(&self) -> Result<(), ValidationError> {
        crate::ProcessRule {
            id: self.id.clone(),
            label: self.label.clone(),
            enabled: self.enabled,
            executable_basenames: self.executable_basenames.clone(),
            executable_paths: self.executable_paths.clone(),
            script_path_suffixes: self.script_path_suffixes.clone(),
            process_names: self.process_names.clone(),
        }
        .validate()
    }
}
// No From<crate::SettingsDto>: its migration defaults have already erased
// omitted-vs-null/value provenance. Backend construction must use actual facts.
