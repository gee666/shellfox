use crate::{validation::*, EnvVar, Validate, ValidationError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterId {
    WindowsTerminal,
    GnomeTerminal,
    EmbeddedPty,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellId {
    Pwsh,
    WindowsPowershell,
    Bash,
    LoginShell,
    Wsl,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalLifetime {
    AppOwned,
    ExternalLegacy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessRule {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub executable_basenames: Vec<String>,
    pub executable_paths: Vec<String>,
    pub script_path_suffixes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_names: Option<Vec<String>>,
}
impl Validate for ProcessRule {
    fn validate(&self) -> Result<(), ValidationError> {
        let custom =
            regex::Regex::new(r"^custom-[a-z0-9][a-z0-9-]*-[a-z0-9]+$").expect("constant regex");
        ensure(
            crate::ids::is_wire_id(&self.id) || self.id.len() <= 120 && custom.is_match(&self.id),
        )?;
        title(&self.label)?;
        ensure(self.executable_basenames.len() + self.executable_paths.len() > 0)?;
        for values in [
            &self.executable_basenames,
            self.process_names.as_ref().unwrap_or(&Vec::new()),
        ] {
            ensure(values.len() <= 30)?;
            for value in values {
                text(value, 260)?;
                ensure(!value.trim().is_empty() && !value.contains(['\\', '/', ':']))?;
            }
        }
        ensure(self.executable_paths.len() <= 30 && self.script_path_suffixes.len() <= 30)?;
        for value in &self.executable_paths {
            cwd(value)?;
            text(value, 32760)?;
        }
        for value in &self.script_path_suffixes {
            text(value, 4096)?;
            ensure(
                !value.trim().is_empty()
                    && !value.split(['/', '\\']).any(|p| p == "." || p == ".."),
            )?;
        }
        Ok(())
    }
}
fn default_background() -> String {
    "#111016".into()
}
fn default_history() -> u32 {
    10
}
fn default_settle() -> f64 {
    24.0
}
fn default_delete() -> f64 {
    720.0
}
fn yes() -> bool {
    true
}

/// Legacy Manager settings, not core::Preferences. Reads preserve host-specific fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsDto {
    pub version: u8,
    #[serde(default)]
    pub python_path: Option<String>,
    pub accent_color: String,
    #[serde(default = "default_background")]
    pub background_color: String,
    pub adapter_id: AdapterId,
    pub shell_id: ShellId,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub shell_executable: Option<String>,
    pub process_rules: Vec<ProcessRule>,
    #[serde(default = "default_history")]
    pub history_page_size: u32,
    #[serde(default = "default_settle")]
    pub session_settle_hours: f64,
    #[serde(default = "default_delete")]
    pub session_delete_hours: f64,
    #[serde(default = "yes")]
    pub auto_update: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    pub terminal_profile_id: Option<Option<String>>,
    #[serde(default)]
    pub terminal_env: Vec<EnvVar>,
}
impl Validate for SettingsDto {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.version == 1 && (1..=100).contains(&self.history_page_size))?;
        ensure(
            self.session_settle_hours.is_finite()
                && (1.0..=87600.0).contains(&self.session_settle_hours),
        )?;
        ensure(
            self.session_delete_hours.is_finite()
                && (1.0..=87600.0).contains(&self.session_delete_hours),
        )?;
        color(&self.accent_color)?;
        color(&self.background_color)?;
        for path in [&self.python_path, &self.shell_executable]
            .into_iter()
            .flatten()
        {
            cwd(path)?;
        }
        if let Some(Some(id)) = &self.terminal_profile_id {
            profile(id)?;
        }
        unique(
            &self
                .process_rules
                .iter()
                .map(|rule| &rule.id)
                .collect::<Vec<_>>(),
            100,
        )?;
        for rule in &self.process_rules {
            rule.validate()?;
        }
        env(&self.terminal_env, false)
    }
}
