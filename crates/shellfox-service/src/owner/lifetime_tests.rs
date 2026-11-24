use super::*;
use crate::local::PrivateRequest;
#[cfg(windows)]
use shellfox_protocol::automation::*;

#[tokio::test]
async fn lifetime_capacity_and_stale_epoch_are_checked_on_actor_dispatch() {
    let data = crate::tests::scratch();
    let mut owner = ServiceOwner::start(
        data.path().into(),
        std::env::current_exe().unwrap(),
        CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    let old = uuid::Uuid::new_v4();
    client.open_lifetime(old).await.unwrap();
    client
        .private_request_in_epoch(PrivateRequest::LocalApi(LocalApiRequest::Snapshot {}), old)
        .await
        .unwrap();
    let new = uuid::Uuid::new_v4();
    client.open_lifetime(new).await.unwrap();
    client.close_lifetime(old).await.unwrap();
    assert!(client
        .private_request_in_epoch(PrivateRequest::LocalApi(LocalApiRequest::Snapshot {}), old)
        .await
        .is_err());
    client
        .private_request_in_epoch(PrivateRequest::LocalApi(LocalApiRequest::Snapshot {}), new)
        .await
        .unwrap();
    for _ in 1..128 {
        owner
            .client(uuid::Uuid::new_v4())
            .open_lifetime(uuid::Uuid::new_v4())
            .await
            .unwrap();
    }
    assert!(owner
        .client(uuid::Uuid::new_v4())
        .open_lifetime(uuid::Uuid::new_v4())
        .await
        .is_err());
    client.close_all_lifetimes().await.unwrap();
    assert!(client
        .private_request_in_epoch(PrivateRequest::LocalApi(LocalApiRequest::Snapshot {}), new)
        .await
        .is_err());
    let cleanup = owner.client(uuid::Uuid::new_v4());
    let cleanup_epoch = uuid::Uuid::new_v4();
    cleanup.open_lifetime(cleanup_epoch).await.unwrap();
    cleanup
        .private_request_in_epoch(
            PrivateRequest::LocalApi(LocalApiRequest::Stop {}),
            cleanup_epoch,
        )
        .await
        .unwrap();
    let retry = owner.client(uuid::Uuid::new_v4());
    let retry_epoch = uuid::Uuid::new_v4();
    retry.open_lifetime(retry_epoch).await.unwrap();
    retry
        .private_request_in_epoch(
            PrivateRequest::LocalApi(LocalApiRequest::Snapshot {}),
            retry_epoch,
        )
        .await
        .unwrap();
    owner.shutdown().await.unwrap();
}
#[cfg(windows)]
#[tokio::test]
#[ignore = "explicit owned raw fixture only; set SHELLFOX_RAW_TEST_HELPER"]
async fn replacement_epoch_cannot_adopt_old_raw_pipes_and_old_eof_cannot_revoke_new_owner() {
    let data = crate::tests::scratch();
    let helper = std::path::PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER").expect("explicit owned raw fixture"),
    );
    assert!(helper.is_absolute() && helper.is_file());
    let mut owner = ServiceOwner::start(
        data.path().into(),
        helper.clone(),
        CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    let epoch = uuid::Uuid::new_v4();
    client.open_lifetime(epoch).await.unwrap();
    let spec = |id| AutomationStart {
        request_id: id,
        executable: helper.to_str().unwrap().into(),
        args: vec!["eof".into()],
        cwd: data.path().to_str().unwrap().into(),
        env: Default::default(),
        title: None,
        detached: false,
    };
    let old = uuid::Uuid::new_v4();
    client
        .private_request_in_epoch(PrivateRequest::AutomationStart(Box::new(spec(old))), epoch)
        .await
        .unwrap();
    let replacement = uuid::Uuid::new_v4();
    client.open_lifetime(replacement).await.unwrap();
    assert!(client
        .private_request_in_epoch(
            PrivateRequest::AutomationRead(AutomationRead { id: old, after: 0 }),
            replacement
        )
        .await
        .is_err());
    assert!(client
        .private_request_in_epoch(
            PrivateRequest::AutomationWrite(AutomationWrite {
                id: old,
                data: String::new(),
                eof: true
            }),
            replacement
        )
        .await
        .is_err());
    let fresh = uuid::Uuid::new_v4();
    client
        .private_request_in_epoch(
            PrivateRequest::AutomationStart(Box::new(spec(fresh))),
            replacement,
        )
        .await
        .unwrap();
    client.close_lifetime(epoch).await.unwrap(); // Old EOF cannot revoke new binding.
    client
        .private_request_in_epoch(
            PrivateRequest::AutomationWrite(AutomationWrite {
                id: fresh,
                data: String::new(),
                eof: true,
            }),
            replacement,
        )
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let list: AutomationListing = serde_json::from_value(
            client
                .private_request_in_epoch(
                    PrivateRequest::AutomationList(AutomationList {}),
                    replacement,
                )
                .await
                .unwrap(),
        )
        .unwrap();
        if [old, fresh].iter().all(|id| {
            list.0.iter().any(|run| {
                run.id == *id
                    && run.state == AutomationState::Exited
                    && run.exit_code == Some(0)
                    && run.signal.is_none()
            })
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "replacement failed natural EOF/lifetime isolation"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    client.close_all_lifetimes().await.unwrap();
    owner.shutdown().await.unwrap();
}

/// Reopening right after shutdown works, also while other threads fork
/// children (as terminal spawns do) that briefly hold a copy of the store lock.
#[tokio::test]
async fn the_store_reopens_after_every_shutdown() {
    let data = crate::tests::scratch();
    #[cfg(unix)]
    let _forks = Forks::start();
    for round in 0..50 {
        let mut owner = ServiceOwner::start(
            data.path().into(),
            std::env::current_exe().unwrap(),
            CancellationToken::new(),
            (),
        )
        .await
        .unwrap();
        owner
            .client(uuid::Uuid::new_v4())
            .execute(LocalApiRequest::Snapshot {})
            .await
            .unwrap();
        owner.shutdown().await.unwrap();
        if let Err(error) = shellfox_host::Host::open(data.path()) {
            panic!("round {round}: the store is still owned after shutdown: {error:?}");
        }
    }
}
/// Forks children that wait a moment before exec, until dropped.
#[cfg(unix)]
struct Forks(std::sync::Arc<std::sync::atomic::AtomicBool>);
#[cfg(unix)]
impl Forks {
    fn start() -> Self {
        use std::os::unix::process::CommandExt;
        use std::sync::atomic::{AtomicBool, Ordering};
        let running = std::sync::Arc::new(AtomicBool::new(true));
        let forking = running.clone();
        std::thread::spawn(move || {
            while forking.load(Ordering::Relaxed) {
                let mut command = std::process::Command::new("/bin/true");
                // pre_exec makes this a real fork that holds every fd until exec.
                // SAFETY: the closure only sleeps, which is async-signal-safe.
                unsafe {
                    command.pre_exec(|| {
                        std::thread::sleep(Duration::from_millis(5));
                        Ok(())
                    })
                };
                let _ = command.status();
            }
        });
        Self(running)
    }
}
#[cfg(unix)]
impl Drop for Forks {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
