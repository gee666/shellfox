use super::*;
fn host() -> PathBuf {
    std::env::current_dir()
        .unwrap()
        .join("tmp")
        .join(format!("app-plan-{}", shellfox_core::SessionId::new()))
}
#[test]
fn defaults_are_planned_original_control_namespace_and_local_factory_without_effects() {
    assert!(ApplicationArguments::is_flag(std::ffi::OsStr::new(
        "--remote-state-dir"
    )));
    assert!(!ApplicationArguments::is_flag(std::ffi::OsStr::new(
        "--unknown-application-flag"
    )));
    let host = host();
    let config = ApplicationArguments::default().finish(&host).unwrap();
    assert_eq!(
        config.remote_state_dir,
        host.join("rust-service-v1/go-network/node-v1")
    );
    let RemoteBootResources::Planned { hostname, .. } =
        RemoteBootResources::planned_for_current_host(config.remote_state_dir.clone()).unwrap()
    else {
        panic!("read-only plan expected")
    };
    assert_eq!(config.hostname, hostname);
    assert!(!host.exists());
    assert!(!config.remote_state_dir.exists());
}
#[test]
fn explicit_configuration_is_validated_not_inferred_from_recovery_or_installation() {
    let host = host();
    let state = host.join("rust-service-v1/go-network/custom-node");
    let mut args = ApplicationArguments::default();
    args.push("--remote-state-dir", state.clone().into_os_string())
        .unwrap();
    args.push("--remote-hostname", "owned-test-host".into())
        .unwrap();
    assert!(args.push("--remote-hostname", "again".into()).is_err());
    let config = args.finish(&host).unwrap();
    assert_eq!(config.remote_state_dir, state);
    assert_eq!(config.hostname, "owned-test-host");
    assert!(config.validate_separate(&[&state]).is_err());
    for invalid in ["", "-bad", "bad-", "bad.name", "bad/name"] {
        assert!(NativeApplicationConfig::new(&host, None, Some(invalid.into())).is_err());
    }
    assert!(NativeApplicationConfig::new(&host, Some(host.clone()), None).is_err());
    assert!(NativeApplicationConfig::new(&host, Some("relative".into()), None).is_err());
}
#[test]
fn development_install_facts_are_unavailable_and_actual_home_query_is_readonly() {
    let system = system_services(false);
    assert_eq!(
        system.cli_integration(SystemScope::User).unwrap_err().code,
        ErrorCode::NativeUnavailable
    );
    use crate::system::{NativeSystemEnvironment, SystemEnvironment};
    if let Ok(expected) = NativeSystemEnvironment.home() {
        assert_eq!(Path::new(&system.home_directory().unwrap().cwd), expected);
    }
}
#[test]
fn control_server_override_is_typed_loopback_planning_only_and_default_none() {
    let host = host();
    let default = ApplicationArguments::default().finish(&host).unwrap();
    assert!(default.control_server.is_none());
    let mut args = ApplicationArguments::default();
    args.push("--remote-control-url", "http://127.0.0.1:1".into())
        .unwrap();
    assert!(args
        .push("--remote-control-url", "http://127.0.0.1:2".into())
        .is_err());
    let configured = args.finish(&host).unwrap();
    let options = configured
        .remote_options(std::env::current_exe().unwrap(), false)
        .unwrap();
    assert!(matches!(
        options.resources,
        RemoteBootResources::Planned {
            control_url: Some(_),
            ..
        }
    ));
    for value in [
        "https://127.0.0.1:1",
        "http://127.0.0.1:0",
        "http://localhost:1",
        "http://192.0.2.1:1",
        "http://127.0.0.1:1/path",
        "http://user@127.0.0.1:1",
    ] {
        assert!(ApplicationArguments::default()
            .push("--remote-control-url", value.into())
            .is_err());
    }
    assert!(!host.exists());
    assert!(!configured.remote_state_dir.exists());
}
#[test]
fn trusted_resource_input_reaches_boot_options_without_materializing_storage() {
    let host = host();
    let resources = ApplicationProviderResources::new(
        Arc::new(UnconfiguredInstallResources),
        SystemInstallPolicy {
            allow_user_install: false,
            allow_open: false,
        },
    );
    let configured = ApplicationArguments::default()
        .finish(&host)
        .unwrap()
        .with_provider_resources(resources);
    assert!(configured.provider_resources.is_some());
    // This is only the explicit supervisor planning fixture, never askpass or package authority.
    configured
        .remote_options(std::env::current_exe().unwrap(), false)
        .unwrap();
    assert!(!host.exists());
    assert!(!host.join("rust-service-v1/ssh").exists());
}
