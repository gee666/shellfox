use super::{
    environment::{SystemEnvironment, SystemPlatform},
    error,
    io::{SystemFile, SystemFiles},
    resources::*,
    SystemResult, SystemScope,
};
use shellfox_protocol::{CliIntegration, ErrorCode};
use std::{collections::BTreeMap, path::Path};

pub(super) fn query(
    files: &dyn SystemFiles,
    env: &dyn SystemEnvironment,
    plan: &CliInstallPlan,
    _scope: SystemScope,
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
    let reason = if plan.package_managed {
        Some("Installed with the system package; removal belongs to its package manager.".into())
    } else if !on_path(&plan.directory, env) {
        Some("Add the configured per-user bin directory to PATH in your own shell configuration; no profile was edited.".into())
    } else {
        None
    };
    Ok(CliIntegration {
        supported: true,
        installed,
        command: "shellfox".into(),
        reason,
    })
}

pub(super) fn set(
    files: &dyn SystemFiles,
    env: &dyn SystemEnvironment,
    plan: &CliInstallPlan,
    scope: SystemScope,
    installed: bool,
) -> SystemResult<CliIntegration> {
    platform_plan(plan, env.platform())?;
    if plan.package_managed {
        return if installed {
            query(files, env, plan, scope)
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
    if installed || owned {
        // The ownership marker is written last and removed last.
        let mut ordered: Vec<_> = plan.files.iter().collect();
        ordered.sort_by_key(|file| file.path == plan.marker);
        for file in ordered {
            files.replace(
                &file.path,
                snapshots.get(&file.path).and_then(Option::as_ref),
                installed.then_some(file.bytes.as_slice()),
                file.executable,
            )?;
        }
    }
    query(files, env, plan, scope)
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
        .is_some_and(|value| owns_file(&value.bytes));
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
    Ok(())
}
fn platform_plan(plan: &CliInstallPlan, platform: SystemPlatform) -> SystemResult<()> {
    validate_plan(plan)?;
    match platform {
        SystemPlatform::Windows => return Err(super::native::windows_installer()),
        SystemPlatform::Other => {
            return Err(error(
                ErrorCode::Unsupported,
                "CLI installation is not supported on this platform.",
            ))
        }
        SystemPlatform::Linux | SystemPlatform::Macos => {}
    }
    if !plan.files.iter().any(|file| {
        file.path.file_name().and_then(|name| name.to_str()) == Some("shellfox") && file.executable
    }) {
        return Err(error(
            ErrorCode::Validation,
            "Configured resources do not contain the platform CLI command launcher.",
        ));
    }
    Ok(())
}
fn on_path(directory: &Path, env: &dyn SystemEnvironment) -> bool {
    env.variable("PATH")
        .is_some_and(|value| std::env::split_paths(&value).any(|entry| entry == directory))
}
