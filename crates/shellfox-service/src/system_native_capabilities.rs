use super::{
    cli, error, explorer, linux, NativeSystemBackend, SystemCapability, SystemFacts,
    SystemMutationCapabilities, SystemNativeCapabilities, SystemPlatform, SystemResult,
    SystemScope,
};
use shellfox_protocol::ErrorCode;

pub(super) fn collect(
    backend: &NativeSystemBackend,
    scope: SystemScope,
    facts: &SystemFacts,
) -> SystemNativeCapabilities {
    let common = common_gate(backend, scope);
    let cli = common
        .clone()
        .and_then(|_| facts.cli.as_ref().map(|_| ()).map_err(Clone::clone))
        .and_then(|_| {
            cli::preflight(
                &*backend.files,
                &*backend.registry,
                &*backend.environment,
                &backend.resources.cli_plan(scope)?,
                scope,
            )
        });
    let explorer = |installed| {
        common
            .clone()
            .and_then(|_| facts.explorer.as_ref().map(|_| ()).map_err(Clone::clone))
            .and_then(|_| {
                let launch = backend.resources.desktop_launch()?;
                match backend.environment.platform() {
                    SystemPlatform::Windows => {
                        explorer::preflight(&*backend.registry, &launch, scope)
                    }
                    SystemPlatform::Linux => linux::preflight(
                        &*backend.files,
                        &*backend.registry,
                        &*backend.environment,
                        &*backend.linux_discovery,
                        &backend.resources.cli_plan(scope)?,
                        &launch,
                        (scope, installed),
                    ),
                    _ => Err(error(
                        ErrorCode::Unsupported,
                        "No supported native file-manager installation contract is available.",
                    )),
                }
            })
    };
    SystemNativeCapabilities {
        cli: SystemMutationCapabilities {
            install: capability(cli.clone()),
            uninstall: capability(cli),
        },
        explorer: SystemMutationCapabilities {
            install: capability(explorer(true)),
            uninstall: capability(explorer(false)),
        },
    }
}
fn common_gate(backend: &NativeSystemBackend, scope: SystemScope) -> SystemResult<()> {
    if scope == SystemScope::System {
        return Err(error(ErrorCode::Unsupported,"System integration requires an authorized package/elevation policy; none is configured."));
    }
    if !backend.policy.allow_user_install {
        return Err(error(
            ErrorCode::Unsupported,
            "Per-user installation policy is disabled. No default write is authorized.",
        ));
    }
    backend.resources.validate_current()
}
fn capability(result: SystemResult<()>) -> SystemCapability {
    match result {
        Ok(()) => SystemCapability::Available,
        Err(error) => SystemCapability::Unavailable(error),
    }
}
