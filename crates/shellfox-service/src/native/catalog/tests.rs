use super::*;
use crate::{
    native::{NativeAction, NativeResponse},
    owner::ServiceOwner,
};
#[test]
fn direct_or_lease_shaped_native_call_cannot_bypass_protected_catalog_worker_route() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    assert!(service
        .execute_native(NativeAction::TerminalProfiles, 73)
        .is_err());
    assert!(service.catalog.jobs.is_empty());
    service.shutdown().unwrap();
}
#[tokio::test]
#[ignore = "explicit configured owned supervisor; readonly discovery only, no GUI filesystem claim"]
async fn actual_native_owner_catalog_is_bound_to_its_configured_launch_resources() {
    let data = crate::tests::scratch();
    let helper = std::path::PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER").expect("explicit owned supervisor"),
    );
    assert!(helper.is_absolute() && helper.is_file());
    let mut owner = ServiceOwner::start(data.path().into(), helper, CancellationToken::new(), ())
        .await
        .unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    let NativeResponse::TerminalProfiles(catalog) =
        client.native(NativeAction::TerminalProfiles).await.unwrap()
    else {
        panic!("missing native catalog")
    };
    assert_eq!(
        catalog.probe.platform,
        if cfg!(windows) {
            "win32"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else {
            std::env::consts::OS
        }
    );
    if let Some(default) = &catalog.profiles.default_profile_id {
        assert!(catalog
            .profiles
            .profiles
            .iter()
            .any(|profile| profile.id == *default && profile.available));
    }
    assert!(
        !catalog.probe.capabilities.close_terminal,
        "catalog alone grants no runtime-target operation"
    );
    owner.shutdown().await.unwrap();
}
#[cfg(target_os = "linux")]
#[test]
fn linux_catalog_offers_installed_shells_and_the_legacy_default() {
    use std::{os::unix::fs::PermissionsExt, path::Path};

    let discovery =
        crate::activity::integration::NativeFactsDiscovery::new(std::env::current_exe().unwrap())
            .unwrap();
    let default = shellfox_core::ProfileId::default();
    let catalog = discovery
        .discover_catalog(
            &shellfox_core::Settings::default(),
            &default,
            &CancellationToken::new(),
        )
        .unwrap()
        .profiles;
    assert_eq!(
        catalog.default_profile_id.as_deref(),
        Some(default.as_str())
    );
    for name in ["sh", "bash", "zsh", "fish"] {
        let installed = ["/bin", "/usr/bin", "/usr/local/bin"]
            .into_iter()
            .any(|parent| {
                Path::new(parent)
                    .join(name)
                    .metadata()
                    .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            });
        let profile = catalog.profiles.iter().find(|profile| profile.id == name);
        assert_eq!(profile.is_some(), installed, "{name}");
        if let Some(profile) = profile {
            assert!(profile.available, "{profile:?}");
            assert_eq!(Path::new(&profile.executable).file_name().unwrap(), name);
        }
    }
}
#[test]
fn changed_owner_configuration_cannot_publish_a_stale_successful_catalog() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let (send, mut reply) = oneshot::channel();
    service.enqueue_native_catalog(9, send);
    service
        .host
        .save_global_settings(shellfox_core::SettingsPatch {
            history_page_size: Some(17),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        service.pump_native_catalog();
        if let Ok(result) = reply.try_recv() {
            assert!(result.is_err());
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    service.shutdown().unwrap();
}
#[test]
fn discovery_slots_are_bounded_cancelled_and_original_threads_join_on_stop() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let mut replies = Vec::new();
    for _ in 0..MAX_JOBS {
        let (send, reply) = oneshot::channel();
        service.enqueue_native_catalog(9, send);
        replies.push(reply);
    }
    assert_eq!(service.catalog.jobs.len(), MAX_JOBS);
    let (send, mut rejected) = oneshot::channel();
    service.enqueue_native_catalog(9, send);
    assert!(rejected.try_recv().unwrap().is_err());
    service.revoke_native_catalog(9);
    service.stop_native_catalog().unwrap();
    assert!(service.catalog.jobs.is_empty());
    for mut reply in replies {
        assert!(reply.try_recv().unwrap().is_err());
    }
    service.shutdown().unwrap();
}

#[test]
fn the_effective_default_is_the_saved_one_only_when_this_host_can_run_it() {
    use crate::effective_default_profile as pick;
    assert_eq!(pick("zsh", ["login-shell", "zsh"]), Some("zsh"));
    // 0.9's macOS default, pwsh, falls back to the first usable profile.
    assert_eq!(pick("pwsh", ["login-shell", "zsh"]), Some("login-shell"));
    assert_eq!(pick("pwsh", std::iter::empty()), None);
}
