use shellfox_remote::adapter::filter_session_profiles;
use shellfox_protocol::{SessionDto,Status,Validate};
mod support;
#[test]
fn hidden_elevated_or_unknown_profiles_cannot_leak_agents_counts_or_focus() {
    let fixtures = support::fixture_document();
    let sessions:Vec<SessionDto> = serde_json::from_value(fixtures["statusSessions"].clone()).unwrap();
    let mut session = sessions.into_iter().find(|session|session.counts.agents > 0 && session.settled_at.is_none()).unwrap();
    for tab in &mut session.tabs {tab.profile_id = Some(Some("elevated-unverified".into()));}
    let filtered = filter_session_profiles(session,&["login-shell".to_owned()].into()).unwrap();
    filtered.validate().unwrap();
    assert!(filtered.tabs.is_empty()); assert_eq!(filtered.counts.agents,0); assert_eq!(filtered.counts.running,0);
    assert!(matches!(filtered.status,Status::Unknown)); assert!(!filtered.can_focus);
}
