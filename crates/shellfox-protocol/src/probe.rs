use crate::{validation::*, *};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capabilities {
    pub create_window: bool,
    pub add_tab: bool,
    pub focus_window: bool,
    pub activate_tab: bool,
    pub split_pane: bool,
    pub attach_existing: bool,
    pub close_terminal: bool,
    pub command_exit_status: bool,
    pub process_tracking: bool,
    pub explorer_context_menu: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub embedded_terminal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub terminal_lifetime: Option<TerminalLifetime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub shell_survival: Option<bool>,
}
impl Validate for Capabilities {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(!self.split_pane && !self.attach_existing && !self.command_exit_status)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonProbe {
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub detected: Option<String>,
    pub usable: bool,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellOption {
    pub id: ShellId,
    pub executable: String,
    pub available: bool,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeProbe {
    pub platform: String,
    pub arch: String,
    pub adapter_id: AdapterId,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub python: Option<PythonProbe>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub terminal_version: Option<String>,
    pub capabilities: Capabilities,
    pub shells: Vec<ShellOption>,
    pub reasons: Vec<String>,
}
impl Validate for NativeProbe {
    fn validate(&self) -> Result<(), ValidationError> {
        text(&self.platform, 100)?;
        text(&self.arch, 100)?;
        self.capabilities.validate()?;
        if let Some(version) = &self.terminal_version {
            text(version, 100)?;
        }
        ensure(self.shells.len() <= 100 && self.reasons.len() <= 100)?;
        for shell in &self.shells {
            cwd(&shell.executable)?;
            if let Some(reason) = &shell.reason {
                text(reason, 1000)?;
            }
        }
        for reason in &self.reasons {
            text(reason, 1000)?;
        }
        if let Some(python) = &self.python {
            if let Some(path) = &python.detected {
                cwd(path)?;
            }
            if let Some(reason) = &python.reason {
                text(reason, 1000)?;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplorerIntegration {
    pub supported: bool,
    pub installed: bool,
    pub folder_item_installed: bool,
    pub background_installed: bool,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliIntegration {
    pub supported: bool,
    pub installed: bool,
    pub command: String,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub reason: Option<String>,
}
