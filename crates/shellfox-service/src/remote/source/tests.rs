use super::*;
use crate::activity::integration::SourceFactsPublisher;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Projector {
    calls: AtomicUsize,
    pause: bool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl SourceFactsProjector for Projector {
    fn project(
        &self,
        source: Arc<SourceFacts>,
        request: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_> {
        Box::pin(async move {
            assert!(source.matches_request(&request));
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            if self.pause {
                self.release.notified().await;
            }
            // This test supplies no discovery evidence, so no ManagerFacts may
            // be manufactured merely because an owner watch became available.
            Err(capability_unavailable("test discovery evidence pending"))
        })
    }
}
fn projector(pause: bool) -> Arc<Projector> {
    Arc::new(Projector {
        calls: AtomicUsize::new(0),
        pause,
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    })
}
fn request() -> ManagerFactsRequest {
    ManagerFactsRequest {
        target_session: None,
        private_tabs: Default::default(),
        snapshot: Snapshot {
            revision: 1,
            sessions: vec![],
            preferences: Default::default(),
            selection: Default::default(),
        },
        settings: Default::default(),
        terminals: vec![],
        cancelled: wire::CancellationToken::new(),
    }
}
fn publish(publisher: &mut SourceFactsPublisher, request: &ManagerFactsRequest) {
    publisher
        .publish(
            request.snapshot.clone(),
            request.settings.clone(),
            vec![],
            wire::CancellationToken::new(),
        )
        .unwrap();
}
#[tokio::test]
async fn slot_is_pending_until_actual_watch_has_evidence_and_rejects_rebinding() {
    let projector = projector(false);
    let slot = Arc::new(SourceFactsProviderSlot::new(projector.clone()));
    let request = request();
    assert_eq!(
        slot.collect(request.clone()).await.err().unwrap().code,
        ErrorCode::Unsupported
    );
    let (mut publisher, watch) = SourceFactsPublisher::new();
    let installer: SourceFactsInstaller = slot.installer();
    installer(watch.clone()).unwrap();
    assert!(installer(watch).is_err());
    assert_eq!(
        slot.collect(request.clone()).await.err().unwrap().code,
        ErrorCode::Unsupported
    );
    assert_eq!(projector.calls.load(Ordering::SeqCst), 0);
    publish(&mut publisher, &request);
    let error = slot.collect(request).await.err().unwrap();
    assert!(error.message.contains("test discovery evidence pending"));
    assert_eq!(projector.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn metadata_settings_and_runtime_fences_are_checked_before_projection() {
    let projector = projector(false);
    let slot = SourceFactsProviderSlot::new(projector.clone());
    let original = request();
    let (mut publisher, watch) = SourceFactsPublisher::new();
    publish(&mut publisher, &original);
    slot.install(watch).unwrap();
    let mut changed = original.clone();
    changed.snapshot.revision += 1;
    assert_eq!(
        slot.collect(changed).await.err().unwrap().code,
        ErrorCode::TargetLost
    );
    let mut changed = original.clone();
    changed.settings.auto_update = !changed.settings.auto_update;
    assert_eq!(
        slot.collect(changed).await.err().unwrap().code,
        ErrorCode::TargetLost
    );
    let mut changed = original;
    changed.terminals.push(RuntimeFactIdentity {
        identity: shellfox_runtime::Identity {
            session_id: SessionId::new(),
            tab_id: shellfox_core::TabId::new(),
            generation: shellfox_core::Generation::new(),
        },
        phase: shellfox_runtime::Phase::Active,
    });
    assert_eq!(
        slot.collect(changed).await.err().unwrap().code,
        ErrorCode::TargetLost
    );
    assert_eq!(projector.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn watch_borrow_is_released_and_replacement_evidence_invalidates_async_result() {
    let projector = projector(true);
    let slot = SourceFactsProviderSlot::new(projector.clone());
    let original = request();
    let (mut publisher, watch) = SourceFactsPublisher::new();
    publish(&mut publisher, &original);
    slot.install(watch).unwrap();
    let (result, ()) = tokio::join!(slot.collect(original.clone()), async {
        projector.entered.notified().await;
        publish(&mut publisher, &original);
        projector.release.notify_one();
    });
    assert_eq!(result.err().unwrap().code, ErrorCode::TargetLost);
}
#[tokio::test]
async fn cancellation_and_closed_or_invalidated_source_never_reuse_old_facts() {
    let projector = projector(false);
    let slot = SourceFactsProviderSlot::new(projector.clone());
    let request = request();
    let (mut publisher, watch) = SourceFactsPublisher::new();
    publish(&mut publisher, &request);
    slot.install(watch).unwrap();
    publisher.invalidate();
    assert_eq!(
        slot.collect(request.clone()).await.err().unwrap().code,
        ErrorCode::Unsupported
    );
    request.cancelled.cancel();
    assert_eq!(
        slot.collect(request).await.err().unwrap().code,
        ErrorCode::NativeUnavailable
    );
    assert_eq!(projector.calls.load(Ordering::SeqCst), 0);
    let (publisher, watch) = SourceFactsPublisher::new();
    drop(publisher);
    assert!(SourceFactsProviderSlot::new(projector)
        .install(watch)
        .is_err());
}
