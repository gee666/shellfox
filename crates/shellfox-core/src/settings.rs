//! Version-one global settings shared by desktop and remote clients.
//! Device fonts, layout, swatches and last-used directories do not belong here.
use crate::{
    settings_rules::default_process_rules, validation, EnvVar, Preferences, ProfileId, Result,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsAdapter {
    WindowsTerminal,
    GnomeTerminal,
    EmbeddedPty,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsShell {
    Pwsh,
    WindowsPowershell,
    Bash,
    LoginShell,
    Wsl,
}
impl SettingsShell {
    pub fn profile_name(self) -> &'static str {
        match self {
            Self::Pwsh => "pwsh",
            Self::WindowsPowershell => "windows-powershell",
            Self::Bash | Self::LoginShell => "login-shell",
            Self::Wsl => "wsl",
        }
    }
}
fn background() -> String {
    "#111016".into()
}
fn history() -> u32 {
    10
}
fn settle() -> f64 {
    24.0
}
fn delete() -> f64 {
    720.0
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    #[serde(deserialize_with = "crate::settings_validation::version")]
    pub version: u8,
    #[serde(default)]
    pub python_path: Option<String>,
    pub accent_color: String,
    #[serde(default = "background")]
    pub background_color: String,
    pub adapter_id: SettingsAdapter,
    pub shell_id: SettingsShell,
    #[serde(deserialize_with = "required_nullable")]
    pub shell_executable: Option<String>,
    pub process_rules: Vec<crate::SettingsProcessRule>,
    #[serde(
        default = "history",
        deserialize_with = "crate::settings_validation::history"
    )]
    pub history_page_size: u32,
    #[serde(default = "settle")]
    pub session_settle_hours: f64,
    #[serde(default = "delete")]
    pub session_delete_hours: f64,
    #[serde(default = "yes")]
    pub auto_update: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_nullable"
    )]
    pub terminal_profile_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::settings_validation::environment")]
    pub terminal_env: Vec<EnvVar>,
}
fn required_nullable<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::deserialize(de)
}
pub(crate) fn present_nullable<'de, D, T>(de: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(de).map(Some)
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            python_path: None,
            accent_color: "#ec4899".into(),
            background_color: background(),
            adapter_id: if cfg!(target_os = "linux") {
                SettingsAdapter::GnomeTerminal
            } else {
                SettingsAdapter::WindowsTerminal
            },
            shell_id: if cfg!(target_os = "linux") {
                SettingsShell::Bash
            } else {
                SettingsShell::Pwsh
            },
            shell_executable: None,
            terminal_profile_id: None,
            terminal_env: vec![],
            process_rules: default_process_rules(),
            history_page_size: history(),
            session_settle_hours: settle(),
            session_delete_hours: delete(),
            auto_update: true,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        validation::ensure(self.version == 1, "Unsupported settings version.")?;
        validation::color(&self.accent_color)?;
        validation::color(&self.background_color)?;
        for path in [&self.python_path, &self.shell_executable]
            .into_iter()
            .flatten()
        {
            crate::settings_validation::path(path, 32767)?;
        }
        if let Some(Some(profile)) = &self.terminal_profile_id {
            crate::settings_validation::profile(profile)?;
        }
        validation::ensure(
            (1..=100).contains(&self.history_page_size),
            "Invalid history page size.",
        )?;
        for hours in [self.session_settle_hours, self.session_delete_hours] {
            validation::ensure(
                hours.is_finite() && (1.0..=87600.0).contains(&hours),
                "Invalid session retention hours.",
            )?;
        }
        validation::ensure(self.process_rules.len() <= 100, "Too many process rules.")?;
        let mut ids = std::collections::HashSet::new();
        for rule in &self.process_rules {
            rule.validate()?;
            validation::ensure(ids.insert(&rule.id), "Duplicate process rule ID.")?;
        }
        crate::validate_env(&self.terminal_env, false)
    }
    /// Explicit profiles are opaque metadata, including unavailable WSL entries.
    /// Only an absent/null profile falls back to the configured shell intent.
    pub fn project_preferences(&self, local: &Preferences) -> Result<Preferences> {
        let profile = ProfileId::new(
            self.terminal_profile_id
                .as_ref()
                .and_then(Option::as_deref)
                .unwrap_or(self.shell_id.profile_name()),
        )?;
        Ok(Preferences {
            accent_color: self.accent_color.clone(),
            background_color: self.background_color.clone(),
            default_profile_id: profile,
            ..local.clone()
        })
    }
    pub fn patched(&self, patch: SettingsPatch) -> Result<Self> {
        let mut next = self.clone();
        macro_rules! apply { ($($field:ident),* $(,)?) => { $(if let Some(value) = patch.$field { next.$field = value; })* }; }
        apply!(
            version,
            python_path,
            accent_color,
            background_color,
            adapter_id,
            shell_id,
            shell_executable,
            process_rules,
            history_page_size,
            session_settle_hours,
            session_delete_hours,
            auto_update
        );
        if let Some(env) = patch.terminal_env {
            next.terminal_env = crate::normalize_env(env, false)?;
        }
        if let Some(profile) = patch.terminal_profile_id {
            next.terminal_profile_id = Some(profile);
        }
        next.validate()?;
        Ok(next)
    }
}

/// Missing fields preserve the latest committed value. Null is legal only for
/// pythonPath, shellExecutable and terminalProfileId, where it clears the value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    #[serde(
        default,
        deserialize_with = "crate::settings_validation::version_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub version: Option<u8>,
    #[serde(
        default,
        deserialize_with = "present_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub python_path: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "not_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub accent_color: Option<String>,
    #[serde(
        default,
        deserialize_with = "not_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub background_color: Option<String>,
    #[serde(
        default,
        deserialize_with = "not_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub adapter_id: Option<SettingsAdapter>,
    #[serde(
        default,
        deserialize_with = "not_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub shell_id: Option<SettingsShell>,
    #[serde(
        default,
        deserialize_with = "present_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub shell_executable: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub terminal_profile_id: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "not_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub process_rules: Option<Vec<crate::SettingsProcessRule>>,
    #[serde(
        default,
        deserialize_with = "crate::settings_validation::history_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub history_page_size: Option<u32>,
    #[serde(
        default,
        deserialize_with = "not_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub session_settle_hours: Option<f64>,
    #[serde(
        default,
        deserialize_with = "not_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub session_delete_hours: Option<f64>,
    #[serde(
        default,
        deserialize_with = "not_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_update: Option<bool>,
    #[serde(
        default,
        deserialize_with = "crate::settings_validation::environment_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub terminal_env: Option<Vec<EnvVar>>,
}
fn not_null<'de, D, T>(de: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(de).map(Some)
}
