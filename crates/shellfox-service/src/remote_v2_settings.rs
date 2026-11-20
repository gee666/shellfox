//! V2 settings are built from actual owner configuration, not defaulted v1 DTOs.
use super::*;
pub(super) fn project(settings: &shellfox_core::Settings) -> v2::Settings {
    v2::Settings {
        version: settings.version,
        python_path: None,
        accent_color: settings.accent_color.clone(),
        background_color: Some(settings.background_color.clone()),
        adapter_id: match settings.adapter_id {
            shellfox_core::SettingsAdapter::EmbeddedPty => wire::AdapterId::EmbeddedPty,
            shellfox_core::SettingsAdapter::GnomeTerminal => wire::AdapterId::GnomeTerminal,
            shellfox_core::SettingsAdapter::WindowsTerminal => wire::AdapterId::WindowsTerminal,
        },
        shell_id: match settings.shell_id {
            shellfox_core::SettingsShell::Pwsh => wire::ShellId::Pwsh,
            shellfox_core::SettingsShell::WindowsPowershell => wire::ShellId::WindowsPowershell,
            shellfox_core::SettingsShell::Bash => wire::ShellId::Bash,
            shellfox_core::SettingsShell::LoginShell => wire::ShellId::LoginShell,
            shellfox_core::SettingsShell::Wsl => wire::ShellId::Wsl,
        },
        shell_executable: settings.shell_executable.clone(),
        process_rules: settings
            .process_rules
            .iter()
            .map(|rule| v2::ProcessRule {
                id: rule.id.clone(),
                label: rule.label.clone(),
                enabled: rule.enabled,
                executable_basenames: rule.executable_basenames.clone(),
                executable_paths: rule.executable_paths.clone(),
                script_path_suffixes: rule.script_path_suffixes.clone(),
                process_names: rule.process_names.clone(),
            })
            .collect(),
        history_page_size: Some(settings.history_page_size),
        session_settle_hours: Some(settings.session_settle_hours),
        session_delete_hours: Some(settings.session_delete_hours),
        auto_update: Some(settings.auto_update),
        terminal_profile_id: settings.terminal_profile_id.clone(),
        terminal_env: Some(settings.terminal_env.clone()),
    }
}
