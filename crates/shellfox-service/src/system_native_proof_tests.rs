use super::super::*;
use super::support::Fixture;
use shellfox_protocol::{CancellationToken, ErrorCode};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct Authority;
impl SystemNativeAuthority for Authority {
    fn validate_system_controller(&self, controller: u64) -> SystemResult<()> {
        if controller == 41 {
            Ok(())
        } else {
            Err(error(
                ErrorCode::AuthFailed,
                "Not an original native controller.",
            ))
        }
    }
}
struct Resources {
    valid: AtomicBool,
    inner: Arc<ConfiguredSystemResources>,
}
impl SystemInstallResources for Resources {
    fn cli_plan(&self, scope: SystemScope) -> SystemResult<CliInstallPlan> {
        self.inner.cli_plan(scope)
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        self.inner.desktop_launch()
    }
    fn validate_current(&self) -> SystemResult<()> {
        if !self.valid.load(Ordering::SeqCst) {
            return Err(error(
                ErrorCode::NativeUnavailable,
                "Injected external package proof expired.",
            ));
        }
        self.inner.validate_current()
    }
}
#[test]
fn missing_external_resource_proof_allows_queries_but_never_installation() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let mut backend = fixture.backend(
        Vec::new(),
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
    );
    Arc::get_mut(&mut backend).unwrap().resources = Arc::new(Resources {
        valid: AtomicBool::new(false),
        inner: fixture.resources.clone(),
    });
    let handler = SystemNativeHandler::new(backend, CancellationToken::new()).unwrap();
    let SystemNativeResponse::Status(status) = handler
        .handle(
            41,
            SystemNativeRequest::Status {
                scope: SystemScope::User,
            },
            &Authority,
        )
        .unwrap()
    else {
        panic!("status");
    };
    assert!(status.facts.home.is_ok());
    assert_eq!(
        status.facts.cli.unwrap_err().code,
        ErrorCode::NativeUnavailable
    );
    assert!(matches!(
        status.capabilities.cli.install,
        SystemCapability::Unavailable(_)
    ));
    assert_eq!(
        handler
            .handle(
                41,
                SystemNativeRequest::SetCli {
                    scope: SystemScope::User,
                    installed: true
                },
                &Authority
            )
            .unwrap_err()
            .code,
        ErrorCode::NativeUnavailable
    );
    assert_eq!(fixture.registry.writes.load(Ordering::SeqCst), 0);
    assert!(!fixture.plan.directory.exists());
}
#[test]
fn expired_resource_proof_stays_an_error_and_cannot_enable_mutations() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let resources = Arc::new(Resources {
        valid: AtomicBool::new(true),
        inner: fixture.resources.clone(),
    });
    let mut backend = fixture.backend(
        Vec::new(),
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
    );
    Arc::get_mut(&mut backend).unwrap().resources = resources.clone();
    let handler = SystemNativeHandler::new(backend, CancellationToken::new()).unwrap();
    resources.valid.store(false, Ordering::SeqCst);
    let SystemNativeResponse::Status(status) = handler
        .handle(
            41,
            SystemNativeRequest::Status {
                scope: SystemScope::User,
            },
            &Authority,
        )
        .unwrap()
    else {
        panic!("status")
    };
    assert!(status.facts.home.is_ok());
    assert_eq!(
        status.facts.cli.as_ref().unwrap_err().code,
        ErrorCode::NativeUnavailable
    );
    assert!(
        matches!(status.capabilities.cli.install,SystemCapability::Unavailable(ref error)if error.code==ErrorCode::NativeUnavailable)
    );
    assert_eq!(
        handler
            .handle(
                41,
                SystemNativeRequest::SetCli {
                    scope: SystemScope::User,
                    installed: true
                },
                &Authority
            )
            .unwrap_err()
            .code,
        ErrorCode::NativeUnavailable
    );
    assert!(!fixture.plan.directory.exists());
}
#[test]
fn linux_settings_route_uses_existing_menu_provider_and_no_chooser_or_command() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let handler = SystemNativeHandler::new(
        fixture.backend(
            vec![FileManager::Thunar],
            SystemInstallPolicy {
                allow_user_install: true,
                allow_open: false,
            },
        ),
        CancellationToken::new(),
    )
    .unwrap();
    let SystemNativeResponse::Explorer(value) = handler
        .handle(
            41,
            SystemNativeRequest::SetExplorer {
                scope: SystemScope::User,
                installed: true,
            },
            &Authority,
        )
        .unwrap()
    else {
        panic!("explorer")
    };
    assert!(value.installed);
    assert!(fixture.root.join("config/Thunar/uca.xml").is_file());
    assert!(fixture.plan.marker.is_file());
    assert!(fixture.opener.0.lock().unwrap().is_empty());
    handler
        .handle(
            41,
            SystemNativeRequest::SetExplorer {
                scope: SystemScope::User,
                installed: false,
            },
            &Authority,
        )
        .unwrap();
}
