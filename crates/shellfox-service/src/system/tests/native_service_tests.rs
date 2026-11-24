use super::super::*;
use super::support::Fixture;
use crate::{
    native::{NativeAction, NativeResponse},
    Service,
};
use shellfox_protocol::ErrorCode;

fn open_service(fixture: &Fixture) -> Service {
    let data = fixture.root.join("service-data");
    std::fs::create_dir(&data).unwrap();
    let supervisor = fixture.root.join("package/Shellfox.exe");
    Service::open_owned(&data, &supervisor).unwrap()
}
#[test]
fn actual_service_slot_is_unavailable_until_real_provider_registration() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let mut service = open_service(&fixture);
    assert_eq!(
        service
            .handle_system_native(
                SystemNativeRequest::Status {
                    scope: SystemScope::User
                },
                73
            )
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    let backend = fixture.backend(Vec::new(), SystemInstallPolicy::default());
    service.install_system_native(backend.clone()).unwrap();
    assert_eq!(
        service.install_system_native(backend).unwrap_err().code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        service
            .handle_system_native(
                SystemNativeRequest::Status {
                    scope: SystemScope::User
                },
                73
            )
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
}
#[test]
fn real_native_dispatch_requires_owner_admission_not_a_nonzero_or_pty_identity() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let mut service = open_service(&fixture);
    service
        .install_system_native(fixture.backend(
            Vec::new(),
            SystemInstallPolicy {
                allow_user_install: true,
                allow_open: false,
            },
        ))
        .unwrap();
    assert!(service
        .execute_native(
            NativeAction::System(SystemNativeRequest::Status {
                scope: SystemScope::User
            }),
            73
        )
        .is_err());
    // Ordinary private metadata and non-System native work may use
    // the same numeric controller but cannot mint System authority.
    service
        .execute(crate::local_api::LocalApiRequest::Snapshot {}, 73)
        .unwrap();
    service.execute_native(NativeAction::Status, 73).unwrap();
    assert_eq!(
        service
            .handle_system_native(
                SystemNativeRequest::Status {
                    scope: SystemScope::User
                },
                73
            )
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
    service.admit_system_native(73).unwrap();
    let NativeResponse::System(SystemNativeResponse::Status(status)) = service
        .execute_native(
            NativeAction::System(SystemNativeRequest::Status {
                scope: SystemScope::User,
            }),
            73,
        )
        .unwrap()
    else {
        panic!("real system response")
    };
    assert!(!status.facts.cli.as_ref().unwrap().installed);
    let NativeResponse::System(SystemNativeResponse::Cli(state)) = service
        .execute_native(
            NativeAction::System(SystemNativeRequest::SetCli {
                scope: SystemScope::User,
                installed: true,
            }),
            73,
        )
        .unwrap()
    else {
        panic!("real CLI response")
    };
    assert!(state.installed);
    service.execute_native(NativeAction::DetachAll, 73).unwrap();
    assert!(service
        .execute_native(
            NativeAction::System(SystemNativeRequest::Status {
                scope: SystemScope::User
            }),
            73
        )
        .is_err());
}
#[test]
fn registration_and_requests_bind_the_actual_service_loss_and_stop_fences() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let mut service = open_service(&fixture);
    service
        .install_system_native(fixture.backend(
            Vec::new(),
            SystemInstallPolicy {
                allow_user_install: true,
                allow_open: false,
            },
        ))
        .unwrap();
    service.admit_system_native(73).unwrap();
    service.ownership_lost.cancel();
    assert_eq!(
        service
            .handle_system_native(
                SystemNativeRequest::SetCli {
                    scope: SystemScope::User,
                    installed: true
                },
                73
            )
            .unwrap_err()
            .code,
        ErrorCode::NativeUnavailable
    );
    assert!(!fixture.plan.directory.exists());
    let fixture = Fixture::new(SystemPlatform::Linux);
    let mut service = open_service(&fixture);
    service
        .install_system_native(fixture.backend(
            Vec::new(),
            SystemInstallPolicy {
                allow_user_install: true,
                allow_open: false,
            },
        ))
        .unwrap();
    service.admit_system_native(73).unwrap();
    service.shutdown().unwrap();
    assert!(service.admit_system_native(73).is_err());
    assert!(service
        .handle_system_native(
            SystemNativeRequest::Status {
                scope: SystemScope::User
            },
            73
        )
        .is_err());
}
#[test]
fn native_controller_registry_is_bounded_and_revocation_releases_only_that_grant() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let mut service = open_service(&fixture);
    service
        .install_system_native(fixture.backend(Vec::new(), SystemInstallPolicy::default()))
        .unwrap();
    for controller in 1..=128 {
        service.admit_system_native(controller).unwrap();
    }
    assert!(service.admit_system_native(129).is_err());
    service.revoke_system_native(64);
    service.admit_system_native(129).unwrap();
    assert_eq!(
        service
            .handle_system_native(
                SystemNativeRequest::Status {
                    scope: SystemScope::User
                },
                64
            )
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
    service
        .handle_system_native(
            SystemNativeRequest::Status {
                scope: SystemScope::User,
            },
            129,
        )
        .unwrap();
}
