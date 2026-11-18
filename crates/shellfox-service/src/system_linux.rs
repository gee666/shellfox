use super::{
    cli,
    environment::SystemEnvironment,
    error,
    io::{SystemFile, SystemFiles},
    linux_discovery::*,
    linux_templates as template,
    registry::SystemRegistry,
    resources::*,
    thunar, SystemResult, SystemScope,
};
use shellfox_protocol::{ErrorCode, ExplorerIntegration};
use std::path::PathBuf;
struct MenuFile {
    path: PathBuf,
    content: Option<String>,
    executable: bool,
    xml: bool,
}
fn directories(env: &dyn SystemEnvironment) -> SystemResult<(PathBuf, PathBuf)> {
    let home = env.home()?;
    let data = env
        .variable("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let config = env
        .variable("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    super::validation::absolute_path(&data)?;
    super::validation::absolute_path(&config)?;
    Ok((data, config))
}
fn menu_files(
    env: &dyn SystemEnvironment,
    facts: &LinuxMenuDiscovery,
    launch: &DesktopLaunch,
    all: bool,
) -> SystemResult<Vec<MenuFile>> {
    let launcher = launch.cli_start.as_ref().ok_or_else(|| {
        error(
            ErrorCode::Unsupported,
            "Packaged CLI has no validated start <path> file-menu contract.",
        )
    })?;
    super::validation::absolute_path(launcher)?;
    let (data, config) = directories(env)?;
    let mut result = Vec::new();
    let managers = if all {
        vec![
            FileManager::Nautilus,
            FileManager::Nemo,
            FileManager::Dolphin,
            FileManager::Thunar,
            FileManager::Caja,
        ]
    } else {
        facts.managers.clone()
    };
    for manager in managers {
        match manager {
            FileManager::Nautilus => {
                result.push(MenuFile {
                    path: data.join("nautilus-python/extensions/shellfox.py"),
                    content: Some(template::nautilus(launcher)?),
                    executable: false,
                    xml: false,
                });
                if all || facts.nautilus_python != Some(true) {
                    result.push(MenuFile {
                        path: data.join("nautilus/scripts/Open in Shellfox"),
                        content: Some(template::script(launcher, false)?),
                        executable: true,
                        xml: false,
                    });
                }
            }
            FileManager::Nemo => {
                for background in [false, true] {
                    result.push(MenuFile {
                        path: data.join(format!(
                            "nemo/actions/shellfox{}.nemo_action",
                            if background { "-background" } else { "" }
                        )),
                        content: Some(template::nemo(launcher, background)?),
                        executable: false,
                        xml: false,
                    });
                }
            }
            FileManager::Dolphin => {
                for directory in ["kio/servicemenus", "kservices5/ServiceMenus"] {
                    result.push(MenuFile {
                        path: data.join(directory).join("shellfox.desktop"),
                        content: Some(template::dolphin(launcher)?),
                        executable: true,
                        xml: false,
                    });
                }
            }
            FileManager::Caja => result.push(MenuFile {
                path: config.join("caja/scripts/Open in Shellfox"),
                content: Some(template::script(launcher, true)?),
                executable: true,
                xml: false,
            }),
            FileManager::Thunar => result.push(MenuFile {
                path: config.join("Thunar/uca.xml"),
                content: None,
                executable: false,
                xml: true,
            }),
        }
    }
    Ok(result)
}
fn text(file: &SystemFile) -> SystemResult<&str> {
    std::str::from_utf8(&file.bytes).map_err(|_| {
        error(
            ErrorCode::Validation,
            "File-manager configuration is not UTF-8.",
        )
    })
}
fn thunar_command(launch: &DesktopLaunch) -> SystemResult<String> {
    Ok(format!(
        "{} start %f",
        template::shell_quote(
            launch
                .cli_start
                .as_ref()
                .and_then(|path| path.to_str())
                .ok_or_else(|| error(
                    ErrorCode::Unsupported,
                    "Validated file-menu CLI unavailable."
                ))?
        )
    ))
}
fn state(facts: &LinuxMenuDiscovery, installed: bool) -> ExplorerIntegration {
    let reason = if facts.managers.is_empty() {
        Some("No supported file manager found: Files, Nemo, Dolphin, Thunar or Caja.".into())
    } else if facts.managers.contains(&FileManager::Nautilus) {
        Some(if facts.nautilus_python==Some(true){"Restart Files to see the menu item (or log out and in)."}else{"Nautilus Python support is unavailable or unknown. The owned scripts fallback is used; install python3-nautilus for a top-level menu and restart Files."}.into())
    } else {
        None
    };
    ExplorerIntegration {
        supported: !facts.managers.is_empty(),
        installed,
        folder_item_installed: installed,
        background_installed: installed,
        reason,
    }
}
pub(super) fn query(
    files: &dyn SystemFiles,
    registry: &dyn SystemRegistry,
    env: &dyn SystemEnvironment,
    discovery: &dyn SystemLinuxDiscovery,
    plan: &CliInstallPlan,
    launch: &DesktopLaunch,
    scope: SystemScope,
) -> SystemResult<ExplorerIntegration> {
    if scope == SystemScope::System {
        return Err(error(
            ErrorCode::Unsupported,
            "System file-manager integration discovery is package-specific and is not configured.",
        ));
    }
    let facts = discovery.inspect()?;
    if facts.managers.is_empty() {
        return Ok(state(&facts, false));
    }
    let expected = menu_files(env, &facts, launch, false)?;
    let mut installed = true;
    for item in expected {
        let current = files.read(&item.path)?;
        let matches = if let Some(current) = &current {
            if item.xml {
                thunar::installed(text(current)?, &thunar_command(launch)?)?
            } else {
                item.content.as_deref() == Some(text(current)?)
                    && (!item.executable || current.executable)
            }
        } else {
            false
        };
        if !matches
            && facts.nautilus_python.is_none()
            && item.path.ends_with("nautilus/scripts/Open in Shellfox")
        {
            return Err(error(ErrorCode::NativeUnavailable,"Nautilus Python state is unknown and its fallback is absent/stale; installation was not guessed."));
        }
        installed &= matches;
    }
    installed &= cli::query(files, registry, env, plan, scope)?.installed;
    Ok(state(&facts, installed))
}
pub(super) fn set(
    files: &dyn SystemFiles,
    registry: &dyn SystemRegistry,
    env: &dyn SystemEnvironment,
    discovery: &dyn SystemLinuxDiscovery,
    plan: &CliInstallPlan,
    launch: &DesktopLaunch,
    request: (SystemScope, bool),
) -> SystemResult<ExplorerIntegration> {
    let (scope, enabled) = request;
    if scope == SystemScope::System {
        return Err(error(
            ErrorCode::Unsupported,
            "System file-manager installation requires its package/elevation policy.",
        ));
    }
    let facts = discovery.inspect()?;
    if enabled && facts.managers.is_empty() {
        return Err(error(
            ErrorCode::Unsupported,
            "No supported file manager is present.",
        ));
    }
    let changes = prepare_changes(files, env, &facts, launch, enabled)?;
    if enabled {
        cli::set(files, registry, env, plan, scope, true)?;
    }
    for MenuChange {
        item,
        previous,
        content,
    } in changes
    {
        files.replace(
            &item.path,
            previous.as_ref(),
            content.as_deref(),
            item.executable,
        )?;
    }
    if enabled {
        query(files, registry, env, discovery, plan, launch, scope)
    } else {
        Ok(state(&facts, false))
    }
}

struct MenuChange {
    item: MenuFile,
    previous: Option<SystemFile>,
    content: Option<Vec<u8>>,
}
fn prepare_changes(
    files: &dyn SystemFiles,
    env: &dyn SystemEnvironment,
    facts: &LinuxMenuDiscovery,
    launch: &DesktopLaunch,
    enabled: bool,
) -> SystemResult<Vec<MenuChange>> {
    let targets = menu_files(env, facts, launch, !enabled)?;
    let mut changes = Vec::new();
    for item in targets {
        let previous = files.read(&item.path)?;
        let content = if item.xml {
            let previous_text = previous.as_ref().map(text).transpose()?;
            thunar::merge(previous_text, &thunar_command(launch)?, enabled)?.map(String::into_bytes)
        } else {
            if previous
                .as_ref()
                .is_some_and(|file| !owns_file(&item.path, &file.bytes))
            {
                if enabled {
                    return Err(error(ErrorCode::AuthFailed,"A file-manager action belongs to another installation; nothing was changed."));
                } else {
                    continue;
                }
            }
            if enabled {
                item.content.clone().map(String::into_bytes)
            } else {
                None
            }
        };
        // Unchanged shared XML is never rewritten or removed on uninstall.
        if item.xml && content.is_none() {
            continue;
        }
        changes.push(MenuChange {
            item,
            previous,
            content,
        });
    }
    Ok(changes)
}
pub(super) fn preflight(
    files: &dyn SystemFiles,
    registry: &dyn SystemRegistry,
    env: &dyn SystemEnvironment,
    discovery: &dyn SystemLinuxDiscovery,
    plan: &CliInstallPlan,
    launch: &DesktopLaunch,
    request: (SystemScope, bool),
) -> SystemResult<()> {
    let (scope, enabled) = request;
    if scope == SystemScope::System {
        return Err(error(
            ErrorCode::Unsupported,
            "System file-manager changes require their package/elevation policy.",
        ));
    }
    let facts = discovery.inspect()?;
    if enabled && facts.managers.is_empty() {
        return Err(error(
            ErrorCode::Unsupported,
            "No supported file manager is present.",
        ));
    }
    prepare_changes(files, env, &facts, launch, enabled)?;
    if enabled {
        cli::preflight(files, registry, env, plan, scope)?;
    }
    Ok(())
}
