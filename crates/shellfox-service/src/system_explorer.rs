use super::{
    error,
    registry::*,
    resources::{DesktopLaunch, DesktopLaunchDisposition, EXPLORER_OWNER},
    SystemResult, SystemScope,
};
use shellfox_protocol::{ErrorCode, ExplorerIntegration};

pub(super) fn command(launch: &DesktopLaunch, background: bool) -> SystemResult<String> {
    super::resources::validate_desktop_prefix(launch)?;
    if !launch.new_session_contract
        || !launch
            .executable
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        return Err(error(
            ErrorCode::Unsupported,
            "Configured desktop has no validated --new-session --cwd launch contract.",
        ));
    }
    let mut arguments = vec![quote(&launch.executable.to_string_lossy())?];
    for argument in &launch.prefix_arguments {
        arguments.push(quote(argument)?);
    }
    Ok(format!(
        "{} --new-session --cwd \"{}\\.\"{}",
        arguments.join(" "),
        if background { "%V" } else { "%1" },
        if launch.disposition == DesktopLaunchDisposition::NewWindow {
            " --new-window"
        } else {
            ""
        }
    ))
}
fn quote(value: &str) -> SystemResult<String> {
    if value
        .chars()
        .any(|value| value.is_control() || value == '"')
    {
        return Err(error(
            ErrorCode::Validation,
            "Unsafe configured application argument.",
        ));
    }
    let slashes = value
        .chars()
        .rev()
        .take_while(|value| *value == '\\')
        .count();
    Ok(format!("\"{}{}\"", value, "\\".repeat(slashes)))
}
pub(super) fn query(
    registry: &dyn SystemRegistry,
    launch: &DesktopLaunch,
    scope: SystemScope,
) -> SystemResult<ExplorerIntegration> {
    let folder = registry.read(RegistrySlot::Explorer {
        scope,
        background: false,
    })?;
    let background = registry.read(RegistrySlot::Explorer {
        scope,
        background: true,
    })?;
    let folder_item_installed = owned(&folder)
        && folder.text(RegistryField::Command) == Some(command(launch, false)?.as_str());
    let background_installed = owned(&background)
        && background.text(RegistryField::Command) == Some(command(launch, true)?.as_str())
        && background.text(RegistryField::Selection) != Some("Single");
    Ok(ExplorerIntegration {
        supported: true,
        installed: folder_item_installed && background_installed,
        folder_item_installed,
        background_installed,
        reason: Some("Windows 11: use Show more options (Shift+F10) in Explorer.".into()),
    })
}
pub(super) fn set(
    registry: &dyn SystemRegistry,
    launch: &DesktopLaunch,
    scope: SystemScope,
    installed: bool,
) -> SystemResult<ExplorerIntegration> {
    if scope == SystemScope::System {
        return Err(error(
            ErrorCode::Unsupported,
            "System Explorer changes require an authorized elevation policy; none is configured.",
        ));
    }
    let mut changes = Vec::new();
    for background in [false, true] {
        let slot = RegistrySlot::Explorer { scope, background };
        let previous = registry.read(slot)?;
        if previous.exists && (!owned(&previous) || previous.foreign_content) {
            return Err(error(ErrorCode::AuthFailed,"Explorer verb belongs to another installation or contains unrelated entries; nothing was changed."));
        }
        let desired = if installed {
            let mut state = RegistryState {
                exists: true,
                ..Default::default()
            };
            for (field, value) in [
                (RegistryField::Owner, EXPLORER_OWNER.to_owned()),
                (RegistryField::Label, "Open in Shellfox".into()),
                (RegistryField::MenuLabel, "Open in Shellfox".into()),
                (
                    RegistryField::Icon,
                    launch.executable.to_string_lossy().into_owned(),
                ),
                (RegistryField::Command, command(launch, background)?),
            ] {
                state.values.insert(
                    field,
                    RegistryText {
                        value,
                        kind: RegistryStringKind::Plain,
                    },
                );
            }
            if !background {
                state.values.insert(
                    RegistryField::Selection,
                    RegistryText {
                        value: "Single".into(),
                        kind: RegistryStringKind::Plain,
                    },
                );
            }
            Some(state)
        } else {
            None
        };
        changes.push((slot, previous, desired));
    }
    // Legacy migration is an explicit action only, never a query side effect.
    for background in [false, true] {
        let slot = RegistrySlot::LegacyExplorer { scope, background };
        let previous = registry.read(slot)?;
        if previous.text(RegistryField::Owner) == Some("PiManager/native-v1")
            && !previous.foreign_content
        {
            changes.push((slot, previous, None));
        }
    }
    for (slot, previous, desired) in changes {
        registry.compare_replace(slot, &previous, desired.as_ref())?;
    }
    query(registry, launch, scope)
}
pub(super) fn preflight(
    registry: &dyn SystemRegistry,
    launch: &DesktopLaunch,
    scope: SystemScope,
) -> SystemResult<()> {
    if scope == SystemScope::System {
        return Err(error(
            ErrorCode::Unsupported,
            "System Explorer changes require an authorized elevation policy.",
        ));
    }
    for background in [false, true] {
        command(launch, background)?;
        let state = registry.read(RegistrySlot::Explorer { scope, background })?;
        if state.exists && (!owned(&state) || state.foreign_content) {
            return Err(error(
                ErrorCode::AuthFailed,
                "Explorer verb is foreign or contains unrelated entries.",
            ));
        }
    }
    Ok(())
}
fn owned(state: &RegistryState) -> bool {
    state.text(RegistryField::Owner) == Some(EXPLORER_OWNER)
}
