//! A stalled disk delays only the replies whose request changed metadata.
use super::LocalClient;
use crate::{
    local_api::{LocalApiRequest, LocalApiResponse},
    tests::*,
};
use shellfox_core::SettingsPatch;
use shellfox_host::SettingsRequest;
use std::time::Duration;

fn save(size: u32) -> LocalApiRequest {
    LocalApiRequest::Settings {
        request: SettingsRequest::Save(Box::new(SettingsPatch {
            history_page_size: Some(size),
            ..Default::default()
        })),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn reads_answer_while_the_disk_fails_and_a_change_answers_once_it_is_saved() {
    let data = scratch();
    let (helper, digest) = helper();
    let store = data.path().join(shellfox_host::STORE_FILENAME);
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (ready, started) = tokio::sync::oneshot::channel();
            let directory = data.path().to_owned();
            let executable = helper.clone();
            let mut host = tokio::task::spawn_local(async move {
                super::serve_with_ready(&directory, &executable, digest, (), move |_| {
                    let _ = ready.send(());
                })
                .await
            });
            tokio::select! {
                result = &mut host => panic!("host stopped before readiness: {result:?}"),
                ready = tokio::time::timeout(Duration::from_secs(60), started) => {
                    ready.expect("host readiness").expect("readiness callback");
                }
            }
            let client = LocalClient::connect(data.path(), &helper, digest)
                .await
                .expect("client");
            // Every metadata write on disk now fails, as on a stalled disk.
            rusqlite::Connection::open(&store)
                .unwrap()
                .execute_batch("CREATE TRIGGER stalled BEFORE UPDATE ON host_state BEGIN SELECT RAISE(ABORT,'disk stalled'); END;")
                .unwrap();
            let change = client.execute(save(42));
            tokio::pin!(change);
            tokio::select! {
                answer = &mut change => panic!("a change answered before it was saved: {answer:?}"),
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            // Reads keep answering, from memory, with the change already in it.
            let read = LocalApiRequest::Settings {
                request: SettingsRequest::Get {},
            };
            let read = tokio::time::timeout(Duration::from_secs(5), client.execute(read))
                .await
                .expect("a read answers while the disk fails")
                .unwrap();
            assert!(matches!(read, LocalApiResponse::Settings(settings) if settings.history_page_size == 42));
            tokio::time::timeout(Duration::from_secs(5), client.execute(LocalApiRequest::Snapshot {}))
                .await
                .expect("a snapshot answers while the disk fails")
                .unwrap();
            rusqlite::Connection::open(&store)
                .unwrap()
                .execute_batch("DROP TRIGGER stalled")
                .unwrap();
            let saved = tokio::time::timeout(Duration::from_secs(10), change)
                .await
                .expect("the change answers once it is saved")
                .unwrap();
            assert!(matches!(saved, LocalApiResponse::Settings(_)));
            let saved: String = rusqlite::Connection::open(&store)
                .unwrap()
                .query_row("SELECT value FROM global_settings", [], |r| r.get(0))
                .unwrap();
            assert!(saved.contains("\"historyPageSize\":42"), "{saved}");
            assert!(!host.is_finished());
        })
        .await;
}
