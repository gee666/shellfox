use super::lifecycle_current;
use crate::{activity::integration::read_source, Service};
use shellfox_core::{CreateSession, SessionId};

#[test]
fn a_change_clients_read_lifecycle_from_is_published_before_the_next_poll() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    service.pump_activity();
    let first = read_source(&service.source_facts_watch()).unwrap();
    assert!(lifecycle_current(&service, &first));

    service
        .create_application_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: crate::tests::default_catalog_profile(),
        })
        .unwrap();
    assert!(!lifecycle_current(&service, &first));
    // The process poll isn't due; the stale facts are replaced anyway.
    service.activity.next_poll_ms = u64::MAX;
    service.pump_activity();
    let source = read_source(&service.source_facts_watch()).unwrap();
    assert!(source.sequence > first.sequence);
    assert_eq!(source.snapshot.sessions.len(), 1);
    assert!(lifecycle_current(&service, &source));
    // Current facts wait for the poll.
    service.activity.next_poll_ms = u64::MAX;
    service.pump_activity();
    let unchanged = read_source(&service.source_facts_watch()).unwrap();
    assert_eq!(unchanged.sequence, source.sequence);
}
