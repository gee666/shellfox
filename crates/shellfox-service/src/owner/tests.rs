mod facts;
use crate::{local_api::LocalApiRequest as Q, tests::scratch};
use shellfox_core::{CreateSession, ProfileId, SessionId};
use std::time::Duration;

#[test]
fn owner_queue_admission_is_bounded_and_views_share_no_controller_ids() {
    let (sender, _incoming) = std::sync::mpsc::sync_channel(super::CAPACITY);
    let client = super::OwnerClient {
        sender,
        client_id: uuid::Uuid::new_v4(),
        remote: None,
    };
    let mut pending = Vec::new();
    for _ in 0..super::CAPACITY {
        pending.push(client.try_execute(Q::Snapshot {}).unwrap());
    }
    assert!(client.try_execute(Q::Snapshot {}).is_err());
    let next_view = client.fork();
    assert_ne!(client.client_id(), next_view.client_id());
    let mut controllers = std::collections::HashMap::new();
    let data=scratch();
    let mut service=crate::Service::open_owned(data.path(),&std::env::current_exe().unwrap()).unwrap();
    let a = super::controller_id(&mut controllers, &mut service, client.client_id()).unwrap();
    let observer=service.allocate_viewer_id().unwrap();
    let b = super::controller_id(&mut controllers, &mut service, next_view.client_id()).unwrap();
    assert!(a<observer&&observer<b);
    controllers.remove(&client.client_id());
    assert!(super::controller_id(&mut controllers, &mut service, client.client_id()).unwrap() > b);
}

#[test]
fn native_admission_returns_exact_unsent_action_only_before_enqueue() {
    use crate::{
        native::{NativeAction, NativeCommand},
        Attachment, NativeAdmissionError,
    };
    let (sender, incoming) = std::sync::mpsc::sync_channel(1);
    let client = super::OwnerClient {
        sender,
        client_id: uuid::Uuid::new_v4(),
        remote: None,
    };
    let _pending = client
        .try_native(NativeAction::ChangedSince { revision: 0 })
        .unwrap();
    let tab = shellfox_core::TabId::new();
    let attachment = Attachment {
        controller: 1,
        epoch: 1,
        generation: shellfox_core::Generation::new(),
    };
    let marker = "private input is not a queue diagnostic";
    let action = NativeAction::Terminal {
        tab,
        attachment: attachment.clone(),
        command: NativeCommand::Paste(marker.into()),
        input: Some(crate::native::NativeInputContext {
            captured_settlement: shellfox_host::CapturedSettlement::Captured(None),
            user_initiated: true,
            hint: true,
        }),
    };
    let error = client.try_native(action).unwrap_err();
    assert!(!format!("{error:?}").contains(marker));
    let NativeAdmissionError::Full(action) = error else {
        panic!("pre-enqueue Full required");
    };
    assert!(
        matches!(*action, NativeAction::Terminal { command: NativeCommand::Paste(ref text), .. } if text == marker)
    );
    drop(incoming);
    assert!(matches!(
        client.try_native(NativeAction::ChangedSince { revision: 0 }),
        Err(NativeAdmissionError::Stopped(_))
    ));
    let oversized = NativeAction::Terminal {
        tab,
        attachment,
        command: NativeCommand::Paste("x".repeat(65537)),
        input: Some(crate::native::NativeInputContext {
            captured_settlement: shellfox_host::CapturedSettlement::Captured(None),
            user_initiated: true,
            hint: true,
        }),
    };
    assert!(matches!(
        client.try_native(oversized),
        Err(NativeAdmissionError::Invalid(_))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn post_admission_failures_are_not_retryable_queue_errors() {
    use crate::{native::NativeAction, NativeAdmissionError};
    let (sender, incoming) = std::sync::mpsc::sync_channel(1);
    let client = super::OwnerClient {
        sender,
        client_id: uuid::Uuid::new_v4(),
        remote: None,
    };
    let operation = client.native(NativeAction::ChangedSince { revision: 0 });
    let owner = async {
        loop {
            match incoming.try_recv() {
                Ok(super::Task::Native { reply, .. }) => {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "Service owner queue is full; action was not accepted."
                    )));
                    break;
                }
                Ok(_) => panic!("native task required"),
                Err(std::sync::mpsc::TryRecvError::Empty) => tokio::task::yield_now().await,
                Err(error) => panic!("owner channel failed: {error}"),
            }
        }
    };
    let (result, _) = tokio::join!(operation, owner);
    assert!(result
        .unwrap_err()
        .downcast_ref::<NativeAdmissionError>()
        .is_none());
}

#[test]
fn remote_without_owned_manager_is_unsent_not_a_fake_state_or_owner_task() {
    use crate::{native::NativeAction, remote::native::RemoteRequest, NativeAdmissionError};
    let (sender, incoming) = std::sync::mpsc::sync_channel(1);
    let client = super::OwnerClient {
        sender,
        client_id: uuid::Uuid::new_v4(),
        remote: None,
    };
    assert!(
        matches!(client.try_native(NativeAction::Remote(RemoteRequest::State)),
        Err(NativeAdmissionError::Stopped(action)) if matches!(*action, NativeAction::Remote(RemoteRequest::State)))
    );
    assert!(matches!(
        incoming.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert!(matches!(
        client.try_native(NativeAction::Remote(RemoteRequest::Configure {
            enabled: true,
            hostname: "invalid hostname".into(),
        })),
        Err(NativeAdmissionError::Invalid(_))
    ));
    // The same sender/client still admits LOCAL work after remote rejection.
    let _local = client.try_native(NativeAction::Status).unwrap();
    assert!(matches!(incoming.try_recv(), Ok(super::Task::Native { action, .. }) if matches!(*action, NativeAction::Status)));
}

#[tokio::test(flavor = "current_thread")]
async fn accepted_remote_reply_failure_never_becomes_retryable_full() {
    let (reply, receive) = tokio::sync::oneshot::channel();
    let _ = reply.send(Err(shellfox_protocol::AppError {
        code: shellfox_protocol::ErrorCode::NativeUnavailable,
        message: "Remote control queue full or stopped.".into(),
        retryable: false,
    }));
    let error = crate::NativeAccepted::Remote(receive).await.unwrap_err();
    assert!(error
        .downcast_ref::<crate::NativeAdmissionError>()
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_startup_cannot_open_metadata() {
    let data = scratch();
    let lost = shellfox_protocol::CancellationToken::new();
    lost.cancel();
    assert!(super::ServiceOwner::start(
        data.path().to_owned(),
        std::env::current_exe().unwrap(),
        lost,
        ()
    )
    .await
    .is_err());
    assert!(!data.path().join(shellfox_host::STORE_FILENAME).exists());
}

#[cfg(windows)]
#[tokio::test(flavor = "current_thread")]
async fn windows_global_settings_use_installed_shared_discovery_adapter() {
    use crate::native::{NativeAction, NativeResponse};
    use shellfox_core::{SettingsAdapter, SettingsPatch, SettingsShell};
    use shellfox_host::SettingsRequest;
    let profile =
        shellfox_terminal::shell_profile(shellfox_terminal::ShellProfileId::WindowsPowerShell)
            .unwrap();
    let data = scratch();
    let mut owner = super::ServiceOwner::start(
        data.path().to_owned(),
        std::env::current_exe().unwrap(),
        shellfox_protocol::CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    owner.activate().await.unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    let saved = client
        .native(NativeAction::Settings(SettingsRequest::Save(Box::new(
            SettingsPatch {
                shell_id: Some(SettingsShell::WindowsPowershell),
                shell_executable: Some(None),
                terminal_profile_id: Some(Some("windows-powershell".into())),
                ..Default::default()
            },
        ))))
        .await
        .unwrap();
    let NativeResponse::Settings(settings) = saved else {
        panic!("settings response required");
    };
    assert_eq!(settings.adapter_id, SettingsAdapter::EmbeddedPty);
    assert_eq!(settings.shell_id, SettingsShell::WindowsPowershell);
    assert_eq!(
        settings.shell_executable.as_deref(),
        profile.executable().and_then(|path| path.to_str())
    );
    assert!(client
        .native(NativeAction::Settings(SettingsRequest::Save(Box::new(
            SettingsPatch {
                shell_executable: Some(Some(r"C:\unverified-shell.exe".into())),
                ..Default::default()
            }
        ))))
        .await
        .is_err());
    let NativeResponse::Settings(unchanged) = client
        .native(NativeAction::Settings(SettingsRequest::Get {}))
        .await
        .unwrap()
    else {
        panic!("settings response required");
    };
    assert_eq!(settings, unchanged);
    owner.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn owner_waits_do_not_block_async_loss_supervision() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct SlowDispatcher(Arc<AtomicBool>);
    impl crate::OwnerDispatcher for SlowDispatcher {
        fn dispatch_one(&mut self, _: &mut crate::Service) {
            self.0.store(true, Ordering::Release);
            std::thread::sleep(Duration::from_millis(150));
        }
    }
    let data = scratch();
    let entered = Arc::new(AtomicBool::new(false));
    let lost = shellfox_protocol::CancellationToken::new();
    let mut owner = crate::owner::ServiceOwner::start(
        data.path().to_owned(),
        std::env::current_exe().unwrap(),
        lost.clone(),
        SlowDispatcher(entered.clone()),
    )
    .await
    .unwrap();
    owner.activate().await.unwrap();
    while !entered.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let client = owner.client(uuid::Uuid::new_v4());
    let operation = client.execute(Q::CreateSession {
        input: CreateSession {
            request_id: SessionId::new(),
            title: Some("must not commit".into()),
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        },
    });
    let supervision = async {
        tokio::time::timeout(
            Duration::from_millis(80),
            tokio::time::sleep(Duration::from_millis(5)),
        )
        .await
        .unwrap();
        lost.cancel();
    };
    let (result, _) = tokio::join!(operation, supervision);
    assert!(result.is_err());
    owner.shutdown().await.unwrap();
    assert!(shellfox_host::Host::open(data.path())
        .unwrap()
        .list_sessions()
        .unwrap()
        .is_empty());
}
