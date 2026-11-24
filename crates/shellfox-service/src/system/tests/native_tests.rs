use super::super::*;
use super::support::Fixture;
use shellfox_protocol::{CancellationToken, ErrorCode};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

struct Authority {
    controller: u64,
    allowed: AtomicBool,
}
impl Authority {
    fn local(controller: u64) -> Self {
        Self {
            controller,
            allowed: AtomicBool::new(true),
        }
    }
}
impl SystemNativeAuthority for Authority {
    fn validate_system_controller(&self, controller: u64) -> SystemResult<()> {
        if controller != self.controller || !self.allowed.load(Ordering::SeqCst) {
            return Err(error(
                ErrorCode::AuthFailed,
                "Injected native controller is not active.",
            ));
        }
        Ok(())
    }
}
fn handler(
    fixture: &Fixture,
    policy: SystemInstallPolicy,
    lost: CancellationToken,
) -> SystemNativeHandler {
    SystemNativeHandler::new(fixture.backend(vec![FileManager::Nemo], policy), lost).unwrap()
}
#[test]
fn real_provider_status_is_read_only_and_policy_is_not_a_default_grant() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let authority = Authority::local(41);
    let handler = handler(
        &fixture,
        SystemInstallPolicy::default(),
        CancellationToken::new(),
    );
    let SystemNativeResponse::Status(status) = handler
        .handle(
            41,
            SystemNativeRequest::Status {
                scope: SystemScope::User,
            },
            &authority,
        )
        .unwrap()
    else {
        panic!("status")
    };
    assert!(status.facts.home.is_ok());
    assert!(!status.facts.cli.as_ref().unwrap().installed);
    assert!(
        matches!(status.capabilities.cli.install,SystemCapability::Unavailable(ref error)if error.code==ErrorCode::Unsupported)
    );
    assert!(!fixture.plan.directory.exists());
    assert!(fixture.opener.0.lock().unwrap().is_empty());
    assert_eq!(
        handler
            .handle(
                41,
                SystemNativeRequest::SetCli {
                    scope: SystemScope::User,
                    installed: true
                },
                &authority
            )
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}
#[test]
fn authorized_native_actions_use_the_same_actual_provider_and_return_real_state() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let authority = Authority::local(41);
    let handler = handler(
        &fixture,
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
        CancellationToken::new(),
    );
    let SystemNativeResponse::Status(status) = handler
        .handle(
            41,
            SystemNativeRequest::Status {
                scope: SystemScope::User,
            },
            &authority,
        )
        .unwrap()
    else {
        panic!("status")
    };
    assert_eq!(status.capabilities.cli.install, SystemCapability::Available);
    let SystemNativeResponse::Cli(cli) = handler
        .handle(
            41,
            SystemNativeRequest::SetCli {
                scope: SystemScope::User,
                installed: true,
            },
            &authority,
        )
        .unwrap()
    else {
        panic!("cli")
    };
    assert!(cli.installed);
    let SystemNativeResponse::Explorer(explorer) = handler
        .handle(
            41,
            SystemNativeRequest::SetExplorer {
                scope: SystemScope::User,
                installed: true,
            },
            &authority,
        )
        .unwrap()
    else {
        panic!("explorer")
    };
    assert!(explorer.installed);
    let SystemNativeResponse::Status(status) = handler
        .handle(
            41,
            SystemNativeRequest::Status {
                scope: SystemScope::User,
            },
            &authority,
        )
        .unwrap()
    else {
        panic!("status")
    };
    assert!(status.facts.cli.as_ref().unwrap().installed);
    assert!(status.facts.explorer.as_ref().unwrap().installed);
    handler
        .handle(
            41,
            SystemNativeRequest::SetExplorer {
                scope: SystemScope::User,
                installed: false,
            },
            &authority,
        )
        .unwrap();
    handler
        .handle(
            41,
            SystemNativeRequest::SetCli {
                scope: SystemScope::User,
                installed: false,
            },
            &authority,
        )
        .unwrap();
    assert!(!fixture.plan.marker.exists());
}
#[test]
fn unknown_controller_and_lost_host_are_denied_before_discovery_or_writes() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let authority = Authority::local(41);
    let lost = CancellationToken::new();
    let handler = handler(
        &fixture,
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
        lost.clone(),
    );
    for controller in [0, 42] {
        assert_eq!(
            handler
                .handle(
                    controller,
                    SystemNativeRequest::Status {
                        scope: SystemScope::User
                    },
                    &authority
                )
                .unwrap_err()
                .code,
            ErrorCode::AuthFailed
        );
    }
    lost.cancel();
    assert_eq!(
        handler
            .handle(
                41,
                SystemNativeRequest::SetCli {
                    scope: SystemScope::User,
                    installed: true
                },
                &authority
            )
            .unwrap_err()
            .code,
        ErrorCode::NativeUnavailable
    );
    assert!(!fixture.plan.directory.exists());
}
#[test]
fn system_scope_and_foreign_installations_are_honest_unavailable_capabilities() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let authority = Authority::local(41);
    fs::create_dir(&fixture.plan.directory).unwrap();
    fs::write(fixture.plan.directory.join("shellfox"), b"foreign program").unwrap();
    let handler = handler(
        &fixture,
        SystemInstallPolicy {
            allow_user_install: true,
            allow_open: false,
        },
        CancellationToken::new(),
    );
    let SystemNativeResponse::Status(status) = handler
        .handle(
            41,
            SystemNativeRequest::Status {
                scope: SystemScope::User,
            },
            &authority,
        )
        .unwrap()
    else {
        panic!("status")
    };
    assert!(
        matches!(status.capabilities.cli.install,SystemCapability::Unavailable(ref error)if error.code==ErrorCode::AuthFailed)
    );
    assert_eq!(
        handler
            .handle(
                41,
                SystemNativeRequest::SetCli {
                    scope: SystemScope::User,
                    installed: true
                },
                &authority
            )
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
    assert_eq!(
        handler
            .handle(
                41,
                SystemNativeRequest::SetCli {
                    scope: SystemScope::System,
                    installed: true
                },
                &authority
            )
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

mod loss;
