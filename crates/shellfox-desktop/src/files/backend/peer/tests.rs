use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

struct DropFlag(Arc<AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn cancel_abandons_a_chunk_in_flight_at_once() {
    let cancel = EditorCancellation::default();
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = DropFlag(dropped.clone());
    // A chunk request whose peer never answers.
    let chunk = async move {
        let _flag = flag;
        std::future::pending::<Result<()>>().await
    };
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        trigger.cancel();
    });
    let result = tokio::time::timeout(Duration::from_secs(2), until_cancelled(&cancel, chunk))
        .await
        .expect("cancel waited for the chunk");
    assert!(result.is_err());
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn an_earlier_cancel_never_starts_the_request() {
    let cancel = EditorCancellation::default();
    cancel.cancel();
    let started = Arc::new(AtomicBool::new(false));
    let seen = started.clone();
    let result = until_cancelled(&cancel, async move {
        seen.store(true, Ordering::SeqCst);
        Ok(())
    })
    .await;
    assert!(result.is_err());
    assert!(!started.load(Ordering::SeqCst));
}
