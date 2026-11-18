use super::{
    environment::{SystemEnvironment, SystemPlatform},
    error,
    io::{SystemFile, SystemFiles},
    registry::*,
    resources::*,
    SystemResult, SystemScope,
};
use shellfox_protocol::{CliIntegration, ErrorCode};
use std::{collections::BTreeMap, path::Path};

pub(super) fn query(
    files: &dyn SystemFiles,
    registry: &dyn SystemRegistry,
    env: &dyn SystemEnvironment,
    plan: &CliInstallPlan,
    scope: SystemScope,
) -> SystemResult<CliIntegration> {
    platform_plan(plan, env.platform())?;
    let mut installed = true;
    for expected in &plan.files {
        let current = files.read(&expected.path)?;
        if !current.as_ref().is_some_and(|value| {
            value.bytes == expected.bytes && (!expected.executable || value.executable)
        }) {
            installed = false;
        }
    }
    let mut reason = if plan.package_managed {
        Some("Installed with the system package; removal belongs to its package manager.".into())
    } else {
        None
    };
    if installed && env.platform() == SystemPlatform::Windows {
        let state = registry.read(path_slot(scope))?;
        let on_path = state
            .text(RegistryField::Path)
            .is_some_and(|value| contains(value, &plan.directory, env));
        installed = on_path;
        if !on_path {
            reason = Some(
                "CLI files exist but the configured bin directory is not on persisted PATH.".into(),
            );
        }
    } else if env.platform() != SystemPlatform::Windows && !on_posix_path(&plan.directory, env) {
        reason=Some("Add the configured per-user bin directory to PATH in your own shell configuration; no profile was edited.".into());
    }
    Ok(CliIntegration {
        supported: true,
        installed,
        command: "shellfox".into(),
        reason,
    })
}

pub(super) fn set(
    files: &dyn SystemFiles,
    registry: &dyn SystemRegistry,
    env: &dyn SystemEnvironment,
    plan: &CliInstallPlan,
    scope: SystemScope,
    installed: bool,
) -> SystemResult<CliIntegration> {
    platform_plan(plan, env.platform())?;
    if plan.package_managed {
        return if installed {
            query(files, registry, env, plan, scope)
        } else {
            Err(error(ErrorCode::Unsupported,"System package CLI removal must use its package manager; no elevation was attempted."))
        };
    }
    if scope == SystemScope::System {
        return Err(error(
            ErrorCode::Unsupported,
            "System CLI installation has no authorized elevation/package policy.",
        ));
    }
    let CliMutationState { snapshots, owned } = owned_state(files, plan)?;
    let path_state = if env.platform() == SystemPlatform::Windows {
        Some(registry.read(path_slot(scope))?)
    } else {
        None
    };
    if installed {
        let mut ordered: Vec<_> = plan.files.iter().collect();
        ordered.sort_by_key(|file| file.path == plan.marker);
        for file in ordered {
            files.replace(
                &file.path,
                snapshots.get(&file.path).and_then(Option::as_ref),
                Some(&file.bytes),
                file.executable,
            )?;
        }
    }
    if let Some(previous) = path_state {
        let mut desired = previous.clone();
        let raw = previous.values.get(&RegistryField::Path);
        let parts: Vec<_> = raw
            .filter(|value| !value.value.is_empty())
            .map(|value| value.value.split(';').collect())
            .unwrap_or_default();
        let mut remaining: Vec<_> = parts
            .into_iter()
            .filter(|part| !entry_matches(part, &plan.directory, env))
            .map(str::to_owned)
            .collect();
        if installed {
            remaining.push(
                plan.directory
                    .to_str()
                    .ok_or_else(|| error(ErrorCode::Validation, "CLI bin encoding is invalid."))?
                    .to_owned(),
            );
        }
        desired.exists = true;
        desired.values.insert(
            RegistryField::Path,
            RegistryText {
                value: remaining.join(";"),
                kind: raw
                    .map(|value| value.kind)
                    .unwrap_or(RegistryStringKind::Expandable),
            },
        );
        registry.compare_replace(path_slot(scope), &previous, Some(&desired))?;
    }
    if !installed && owned {
        let mut ordered: Vec<_> = plan.files.iter().collect();
        ordered.sort_by_key(|file| file.path == plan.marker);
        for file in ordered {
            files.replace(
                &file.path,
                snapshots.get(&file.path).and_then(Option::as_ref),
                None,
                file.executable,
            )?;
        }
    }
    query(files, registry, env, plan, scope)
}
struct CliMutationState {
    snapshots: BTreeMap<std::path::PathBuf, Option<SystemFile>>,
    owned: bool,
}
fn owned_state(files: &dyn SystemFiles, plan: &CliInstallPlan) -> SystemResult<CliMutationState> {
    let mut snapshots = BTreeMap::new();
    for expected in &plan.files {
        snapshots.insert(expected.path.clone(), files.read(&expected.path)?);
    }
    let owned = snapshots
        .get(&plan.marker)
        .and_then(Option::as_ref)
        .is_some_and(|value| owns_file(&plan.marker, &value.bytes));
    if snapshots.values().any(Option::is_some) && !owned {
        return Err(error(
            ErrorCode::AuthFailed,
            "Existing CLI files belong to another installation and were not changed.",
        ));
    }
    Ok(CliMutationState { snapshots, owned })
}
pub(super) fn preflight(
    files: &dyn SystemFiles,
    registry: &dyn SystemRegistry,
    env: &dyn SystemEnvironment,
    plan: &CliInstallPlan,
    scope: SystemScope,
) -> SystemResult<()> {
    platform_plan(plan, env.platform())?;
    if scope == SystemScope::System || plan.package_managed {
        return Err(error(
            ErrorCode::Unsupported,
            "CLI changes require their system package/elevation policy.",
        ));
    }
    owned_state(files, plan)?;
    if env.platform() == SystemPlatform::Windows {
        registry.read(path_slot(scope))?;
    }
    Ok(())
}
fn platform_plan(plan: &CliInstallPlan, platform: SystemPlatform) -> SystemResult<()> {
    validate_plan(plan)?;
    if platform == SystemPlatform::Other {
        return Err(error(
            ErrorCode::Unsupported,
            "CLI installation is not supported on this platform.",
        ));
    }
    let required = if platform == SystemPlatform::Windows {
        "shellfox.cmd"
    } else {
        "shellfox"
    };
    if !plan.files.iter().any(|file| {
        file.path.file_name().and_then(|name| name.to_str()) == Some(required)
            && (platform == SystemPlatform::Windows || file.executable)
    }) {
        return Err(error(
            ErrorCode::Validation,
            "Configured resources do not contain the platform CLI command launcher.",
        ));
    }
    Ok(())
}
fn path_slot(scope: SystemScope) -> RegistrySlot {
    if scope == SystemScope::User {
        RegistrySlot::UserPath
    } else {
        RegistrySlot::SystemPath
    }
}
fn on_posix_path(directory: &Path, env: &dyn SystemEnvironment) -> bool {
    env.variable("PATH")
        .is_some_and(|value| std::env::split_paths(&value).any(|entry| entry == directory))
}
fn contains(raw: &str, directory: &Path, env: &dyn SystemEnvironment) -> bool {
    raw.split(';')
        .any(|entry| entry_matches(entry, directory, env))
}
fn entry_matches(raw: &str, directory: &Path, env: &dyn SystemEnvironment) -> bool {
    let value = expand(raw.trim().trim_matches('"'), env);
    let expected = directory
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    value
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
        == expected
}
fn expand(raw: &str, env: &dyn SystemEnvironment) -> String {
    let mut result = String::new();
    let mut remaining = raw;
    for _ in 0..128 {
        let Some(start) = remaining.find('%') else {
            result.push_str(remaining);
            return result;
        };
        result.push_str(&remaining[..start]);
        let suffix = &remaining[start + 1..];
        let Some(end) = suffix.find('%') else {
            result.push_str(&remaining[start..]);
            return result;
        };
        let name = &suffix[..end];
        let replacement = env
            .variable(name)
            .and_then(|value| value.into_string().ok());
        result.push_str(
            replacement
                .as_deref()
                .unwrap_or(&remaining[start..start + end + 2]),
        );
        remaining = &suffix[end + 1..];
    }
    result.push_str(remaining);
    result
}
