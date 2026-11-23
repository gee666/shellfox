use super::*;
use shellfox_core::{CreateSession, ProfileId, SessionId};
#[test]
#[ignore = "explicit owned raw/PTy fixture only; Windows native proof"]
fn hidden_raw_does_not_poison_normal_facts_and_show_adds_exact_external_kind() {
    let (scratch, mut owned, helper) = service();
    let service = &mut owned.0;
    let snapshot = service
        .create_application_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("normal owned fixture".into()),
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let tab = snapshot.sessions[0].tabs[0].id;
    service.attach(tab, 71, false).unwrap();
    let raw = service
        .start_automation(input(&helper, scratch.path(), vec!["idle".into()]), 9)
        .unwrap();
    let hidden = service.automation.relays[&raw.id].feed.identity();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        service.pump();
        if let Some(source) = crate::activity::integration::read_source(&service.activity.watch) {
            if source.snapshot.sessions.len() == 1
                && source
                    .terminals
                    .iter()
                    .any(|facts| facts.identity.tab_id == tab)
            {
                assert!(!source
                    .terminals
                    .iter()
                    .any(|facts| facts.identity == hidden));
                assert_eq!(
                    source
                        .terminals
                        .iter()
                        .find(|facts| facts.identity.tab_id == tab)
                        .unwrap()
                        .source_kind,
                    Some(SourceKind::OwnedPty)
                );
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "hidden reservation invalidated normal catalog facts"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    show(service, raw.id);
    service.activity.dirty = true;
    service.activity.next_poll_ms = 0;
    service.pump();
    let source =
        crate::activity::integration::read_source(&service.activity.watch).expect("published facts");
    assert_eq!(source.snapshot.sessions.len(), 2);
    assert!(source
        .terminals
        .iter()
        .any(|facts| facts.identity == hidden
            && facts.source_kind == Some(SourceKind::ExternalBytes)));
    assert!(source.terminals.iter().any(
        |facts| facts.identity.tab_id == tab && facts.source_kind == Some(SourceKind::OwnedPty)
    ));
    assert!(!source.creation_facts.contains_key(&hidden.session_id));
    service.close_tab(hidden.tab_id).unwrap();
    service.close_tab(tab).unwrap();
    service.shutdown().unwrap();
}
