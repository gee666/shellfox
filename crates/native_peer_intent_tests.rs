//! Pure frontend intent mapping. No Runtime eligibility or grant is asserted.
use super::*;
fn catalog(kind:m::Fact<m::SourceKind>)->(m::Snapshot,m::Id){
    let raw:serde_json::Value=serde_json::from_str(include_str!("shellfox-protocol/tests/fixtures/observer_v2/snapshot.json")).unwrap();
    let mut snapshot:m::Snapshot=serde_json::from_value(raw["value"].clone()).unwrap();
    let tab=snapshot.entries.iter_mut().find_map(|entry|match entry{m::CatalogEntry::Session{value}=>value.tabs.first_mut(),_=>None}).unwrap();
    tab.source_kind=kind;tab.facts.generation=m::Fact::Known{value:None};
    tab.facts.lifecycle=m::Fact::Unknown{reason:m::UnknownReason::NotRecorded};
    tab.facts.status=m::Fact::Unknown{reason:m::UnknownReason::Pending};
    let id=tab.id;(snapshot,id)
}
#[test]
fn metadata_only_unknown_may_request_cold_if_free_without_promoting_any_fact(){
    for reason in [m::UnknownReason::NotRecorded,m::UnknownReason::Pending,m::UnknownReason::Unavailable,m::UnknownReason::InvalidEvidence]{
        let(snapshot,tab)=catalog(m::Fact::Unknown{reason});let before=serde_json::to_value(&snapshot).unwrap();
        assert!(matches!(attach_control(&snapshot,tab).unwrap(),m::ControlRequest::IfFree));
        assert_eq!(serde_json::to_value(&snapshot).unwrap(),before);
    }
    let(snapshot,tab)=catalog(m::Fact::NotApplicable);
    assert!(matches!(attach_control(&snapshot,tab).unwrap(),m::ControlRequest::IfFree));
}
#[test]
fn only_known_non_pty_kinds_choose_no_control_intent(){
    for kind in [m::SourceKind::AutomationPipe,m::SourceKind::ExternalBytes]{
        let(snapshot,tab)=catalog(m::Fact::Known{value:kind});let before=serde_json::to_value(&snapshot).unwrap();
        assert!(matches!(attach_control(&snapshot,tab).unwrap(),m::ControlRequest::None));
        assert_eq!(serde_json::to_value(&snapshot).unwrap(),before);
    }
    let(snapshot,tab)=catalog(m::Fact::Known{value:m::SourceKind::Pty});
    assert!(matches!(attach_control(&snapshot,tab).unwrap(),m::ControlRequest::IfFree));
}
#[test]
fn missing_catalog_target_is_not_reinterpreted_as_unknown_or_another_peer(){
    let(snapshot,_)=catalog(m::Fact::Unknown{reason:m::UnknownReason::Pending});
    assert!(attach_control(&snapshot,m::Id::new()).is_err());
}
