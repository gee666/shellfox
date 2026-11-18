use shellfox_core::{Generation, SessionId, TabId};
use shellfox_protocol::observer_v2::{Fact, SourceKind};
use shellfox_runtime::{Identity, Phase};
use shellfox_service::{
    activity_integration::{InspectionScope, OwnerActivity},
    remote_v2::source_kind,
};

#[test]
fn active_phase_program_marker_and_inspection_scope_never_invent_a_registered_source_kind() {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let mut owner = OwnerActivity::new();
    owner
        .watch(
            identity,
            "automation-pipe".into(),
            InspectionScope::Unsupported,
        )
        .unwrap();
    let mut facts = owner.terminal_facts(identity, Phase::Active);
    assert!(matches!(source_kind(Some(&facts)), Fact::Unknown { .. }));
    assert!(matches!(source_kind(None), Fact::Unknown { .. }));
    facts.source_kind = Some(shellfox_runtime::SourceKind::ExternalBytes);
    assert!(matches!(
        source_kind(Some(&facts)),
        Fact::Known {
            value: SourceKind::ExternalBytes
        }
    ));
    facts.source_kind = Some(shellfox_runtime::SourceKind::OwnedPty);
    assert!(matches!(
        source_kind(Some(&facts)),
        Fact::Known {
            value: SourceKind::Pty
        }
    ));
}
