use super::*;
use crate::remote::{ManagerFactsRequest, RuntimeFactIdentity};
use shellfox_core::{Generation, Preferences, Selection, SessionId, TabId};

fn snapshot() -> Snapshot {
    Snapshot {
        revision: 1,
        sessions: vec![],
        preferences: Preferences::default(),
        selection: Selection::default(),
    }
}
fn request(snapshot: Snapshot) -> ManagerFactsRequest {
    ManagerFactsRequest {
        target_session: None,
        private_tabs: Default::default(),
        snapshot,
        settings: Settings::default(),
        terminals: vec![],
        cancelled: CancellationToken::new(),
    }
}
#[test]
fn source_watch_is_immutable_sanitized_and_requires_current_revision_settings_and_cancellation() {
    let (mut publisher, watch) = SourceFactsPublisher::new();
    assert!(read_source(&watch).is_none());
    let input = snapshot();
    let cancel = CancellationToken::new();
    publisher
        .publish(input.clone(), Settings::default(), vec![], cancel.clone())
        .unwrap();
    let source = read_source(&watch).unwrap();
    assert!(source.matches_request(&request(input.clone())));
    assert!(source_is_current(&watch, &source));
    let mut stale = request(input.clone());
    stale.snapshot.revision = 2;
    assert!(!source.matches_request(&stale));
    let mut stale = request(input.clone());
    stale.settings.auto_update = !stale.settings.auto_update;
    assert!(!source.matches_request(&stale));
    let cancelled = request(input.clone());
    cancelled.cancelled.cancel();
    assert!(!source.matches_request(&cancelled));
    let mut changed = input;
    changed.revision = 2;
    publisher
        .publish(changed, Settings::default(), vec![], cancel.clone())
        .unwrap();
    assert!(!source_is_current(&watch, &source));
    assert_eq!(source.snapshot.revision, 1);
    cancel.cancel();
    assert!(read_source(&watch).is_none());
}
#[test]
fn malformed_publication_and_publisher_drop_invalidate_old_sources() {
    let (mut publisher, watch) = SourceFactsPublisher::new();
    publisher
        .publish(
            snapshot(),
            Settings::default(),
            vec![],
            CancellationToken::new(),
        )
        .unwrap();
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    assert!(publisher
        .publish(
            snapshot(),
            Settings::default(),
            vec![TerminalSourceFacts {
                identity,
                phase: Phase::Active,
                source_kind: None,
                startup: crate::activity_integration::StartupEvidence::Pending,
                observation: None,
                busy: false,
                activity_available: false,
            }],
            CancellationToken::new()
        )
        .is_err());
    assert!(read_source(&watch).is_none());
    publisher
        .publish(
            snapshot(),
            Settings::default(),
            vec![],
            CancellationToken::new(),
        )
        .unwrap();
    drop(publisher);
    assert!(read_source(&watch).is_none());
}
#[test]
fn phase_and_generation_changes_cannot_reuse_a_request_and_epochs_never_wrap() {
    let (mut publisher, watch) = SourceFactsPublisher::new();
    publisher
        .publish(
            snapshot(),
            Settings::default(),
            vec![],
            CancellationToken::new(),
        )
        .unwrap();
    let source = read_source(&watch).unwrap();
    let mut changed = request(snapshot());
    changed.terminals.push(RuntimeFactIdentity {
        identity: Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        },
        phase: Phase::Active,
    });
    assert!(!source.matches_request(&changed));
    publisher.sequence = u64::MAX;
    assert!(publisher
        .publish(
            snapshot(),
            Settings::default(),
            vec![],
            CancellationToken::new()
        )
        .is_err());
    assert!(read_source(&watch).is_none());
}
