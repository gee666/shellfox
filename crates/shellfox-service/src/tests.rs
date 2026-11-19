use crate::{
    experiment::{ExperimentRequest as Q, ExperimentResponse as A},
    local::{read_frame, write_frame, LocalClient},
    security::PrivateDirectory,
};
use shellfox_core::{AddTab, CreateSession, ProfileId, SessionId, SettingsPatch};
use shellfox_host::SettingsRequest;
use std::{path::PathBuf, time::Duration};

pub(crate) fn scratch() -> tempfile::TempDir {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("tmp");
    std::fs::create_dir_all(&root).expect("project tmp");
    tempfile::Builder::new()
        .prefix("service-test-")
        .tempdir_in(root)
        .expect("scratch")
}
pub(crate) fn default_catalog_profile() -> ProfileId {
    #[cfg(any(windows, target_os = "linux"))]
    {
        ProfileId::new(
            shellfox_terminal::default_shell_profile()
                .unwrap()
                .id()
                .as_str(),
        )
        .unwrap()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        // These metadata tests do not launch a PTY on platforms without a supervisor.
        ProfileId::default()
    }
}
#[path = "../../test_dev_helper.rs"]
mod dev_helper;

pub(crate) fn helper() -> (PathBuf, [u8; 32]) {
    dev_helper::helper().expect("development Tailscale helper")
}
#[tokio::test(flavor = "current_thread")]
async fn framed_lengths_are_bounded_before_allocation() {
    use tokio::io::AsyncWriteExt;
    let (mut tx, mut rx) = tokio::io::duplex(64);
    tx.write_u32(2_000_000).await.unwrap();
    assert!(read_frame(&mut rx, 1024).await.is_err());
    let (mut tx, mut rx) = tokio::io::duplex(64);
    write_frame(&mut tx, b"hello").await.unwrap();
    assert_eq!(read_frame(&mut rx, 64).await.unwrap(), b"hello");
}
#[tokio::test(flavor = "current_thread")]
async fn missing_helper_cannot_open_metadata_or_endpoint() {
    let data = scratch();
    let missing = data.path().join("missing-helper.exe");
    assert!(
        PrivateDirectory::prepare(data.path(), &missing, [0; 32], true)
            .await
            .is_err()
    );
    assert!(!data.path().join(shellfox_host::STORE_FILENAME).exists());
    assert!(!data.path().join("rust-service-v1").exists());
}
#[test]
fn local_requests_reject_unknown_fields_and_bounds() {
    use shellfox_protocol::Validate;
    assert!(serde_json::from_str::<Q>(r#"{"operation":"snapshot","unexpected":1}"#).is_err());
    let request = Q::RenameSession {
        id: SessionId::new(),
        title: "\0".into(),
    };
    assert!(request.validate().is_err());
    let request = Q::Settings {
        request: SettingsRequest::Save(Box::new(SettingsPatch {
            history_page_size: Some(101),
            ..Default::default()
        })),
    };
    assert!(request.validate().is_err());
}

#[test]
fn failed_startup_retains_generation_until_confirmed_close() {
    let data = scratch();
    let mut service =
        crate::Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let snapshot = service
        .host()
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("failed launch fixture".into()),
            cwd: data
                .path()
                .join("missing-cwd")
                .to_string_lossy()
                .into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let tab = snapshot.sessions[0].tabs[0].id;
    assert!(service.attach(tab, 1, false).is_err());
    assert_eq!(service.active_terminals(), 1);
    assert!(service.host().close_tab(tab).is_err());
    let closed = service.close_tab(tab).unwrap();
    assert!(closed.sessions[0].tabs.is_empty());
    assert_eq!(service.active_terminals(), 0);
    service.shutdown().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn private_server_typed_clients_security_and_metadata() {
    let data = scratch();
    let (helper, digest) = helper();
    let linked = scratch();
    let linked_data = linked.path().join("data");
    std::fs::create_dir(&linked_data).unwrap();
    let original = linked.path().join("unrelated-fixture");
    std::fs::write(&original, b"must not be opened as SQLite").unwrap();
    std::fs::hard_link(&original, linked_data.join(shellfox_host::STORE_FILENAME)).unwrap();
    let error = crate::local::serve_with_ready(&linked_data, &helper, digest, (), |_| {
        panic!("hard-linked store must not publish readiness");
    })
    .await
    .expect_err("hard-linked store must be rejected");
    assert!(format!("{error:#}").contains(shellfox_host::STORE_FILENAME), "{error:#}");
    assert!(!linked_data.join("rust-service-v1/endpoint.json").exists());
    assert_eq!(
        std::fs::read(original).unwrap(),
        b"must not be opened as SQLite"
    );
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let directory = data.path().to_owned();
            let executable = helper.clone();
            let (ready, received) = tokio::sync::oneshot::channel();
            let mut server = tokio::task::spawn_local(async move {
                crate::local::serve_with_ready(&directory, &executable, digest, (), move |_| {
                    let _ = ready.send(());
                })
                .await
            });
            tokio::select! {
                result = &mut server => panic!("host stopped before readiness: {result:?}"),
                ready = tokio::time::timeout(Duration::from_secs(60), received) => {
                    ready.expect("endpoint publication").expect("host readiness callback");
                }
            }
            let endpoint = data.path().join("rust-service-v1/endpoint.json");
            let client = LocalClient::connect(data.path(), &helper, digest)
                .await
                .unwrap();
            let A::Snapshot(initial) = client.execute(Q::Snapshot {}).await.unwrap() else {
                panic!("snapshot");
            };
            assert!(initial.sessions.is_empty());
            let profile = default_catalog_profile();
            let input = CreateSession {
                request_id: SessionId::new(),
                title: Some("owned fixture".into()),
                cwd: data.path().to_string_lossy().into_owned(),
                profile_id: profile.clone(),
            };
            let A::Snapshot(created) = client.execute(Q::CreateSession { input }).await.unwrap()
            else {
                panic!("created");
            };
            let id = created.sessions[0].id;
            client
                .execute(Q::RenameSession {
                    id,
                    title: "renamed".into(),
                })
                .await
                .unwrap();
            client
                .execute(Q::AddTab {
                    id,
                    input: AddTab {
                        title: Some("second".into()),
                        cwd: None,
                        profile_id: profile,
                    },
                })
                .await
                .unwrap();
            client
                .execute(Q::Settings {
                    request: SettingsRequest::Save(Box::new(SettingsPatch {
                        history_page_size: Some(16),
                        ..Default::default()
                    })),
                })
                .await
                .unwrap();
            let (private, bytes) = PrivateDirectory::read_existing(data.path(), &helper, digest)
                .await
                .unwrap();
            let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let address = record["address"].as_str().unwrap();
            // A valid private transport alone is not enough: the random capability
            // must match. Incorrect credentials cause no service mutation or reply.
            let mut stream = crate::platform::connect(address, &private.path)
                .await
                .unwrap();
            let payload = serde_json::to_vec(
                &serde_json::json!({ "secret": "0".repeat(64), "body": [123,125] }),
            )
            .unwrap();
            write_frame(&mut stream, &payload).await.unwrap();
            assert!(read_frame(&mut stream, 4096).await.is_err());
            drop(stream);
            // Contention cannot launch a second host or replace endpoint credentials.
            assert!(crate::local::serve(data.path(), &helper, digest)
                .await
                .is_err());
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&std::fs::read(&endpoint).unwrap())
                    .unwrap(),
                record
            );
            #[cfg(windows)]
            {
                use std::{os::windows::ffi::OsStrExt, ptr};
                use windows_sys::Win32::{
                    Foundation::LocalFree,
                    Security::{
                        Authorization::{
                            GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
                        },
                        DACL_SECURITY_INFORMATION, UNPROTECTED_DACL_SECURITY_INFORMATION,
                    },
                };
                let endpoint_path = private.path.join("endpoint.json");
                let wide: Vec<u16> = endpoint_path
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                let mut descriptor = ptr::null_mut();
                let mut dacl = ptr::null_mut();
                assert_eq!(
                    unsafe {
                        GetNamedSecurityInfoW(
                            wide.as_ptr(),
                            SE_FILE_OBJECT,
                            DACL_SECURITY_INFORMATION,
                            ptr::null_mut(),
                            ptr::null_mut(),
                            &mut dacl,
                            ptr::null_mut(),
                            &mut descriptor,
                        )
                    },
                    0
                );
                // Remove protection without granting another principal access to
                // the live fixture credentials. The policy still rejects it.
                let changed = unsafe {
                    SetNamedSecurityInfoW(
                        wide.as_ptr(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        dacl,
                        ptr::null(),
                    )
                };
                unsafe {
                    LocalFree(descriptor);
                }
                assert_eq!(changed, 0);
                assert!(LocalClient::connect(data.path(), &helper, digest)
                    .await
                    .is_err());
                // Fixture-only repair after deliberately removing protection.
                let verifier = shellfox_transport::PermissionVerifier::new(private.helper.clone());
                private.check(&endpoint_path, false, true, &verifier).await.unwrap();
            }
            let second = LocalClient::connect(data.path(), &helper, digest)
                .await
                .unwrap();
            let A::Snapshot(snapshot) = second.execute(Q::Snapshot {}).await.unwrap() else {
                panic!("snapshot");
            };
            assert_eq!(snapshot.sessions[0].title, "renamed");
            assert_eq!(snapshot.sessions[0].tabs.len(), 2);
            let A::Settings(settings) = second
                .execute(Q::Settings {
                    request: SettingsRequest::Get {},
                })
                .await
                .unwrap()
            else {
                panic!("global settings");
            };
            assert_eq!(settings.history_page_size, 16);
            assert_eq!(snapshot.preferences.font_size, 14.0);
            second.disconnect().await.unwrap();
            client.execute(Q::DeleteSession { id }).await.unwrap();
            assert!(matches!(
                client.execute(Q::Stop {}).await.unwrap(),
                A::Done {}
            ));
            tokio::time::timeout(Duration::from_secs(10), server)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            #[cfg(windows)]
            assert!(!endpoint.exists());
            #[cfg(unix)]
            assert!(endpoint.is_file(), "Preserved artifact is not a live owner");
            // The kernel ownership lock was released, not a stale PID marker.
            let verified = shellfox_transport::VerifiedHelper::new(helper.clone(), digest).unwrap();
            let owner = shellfox_transport::HostOwnership::acquire(
                &verified,
                private.path.parent().unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
            owner.shutdown().await.unwrap();
        })
        .await;
}
