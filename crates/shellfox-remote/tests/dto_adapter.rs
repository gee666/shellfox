mod support;
use shellfox_core as core;
use shellfox_protocol as wire;
use shellfox_remote::adapter::*;
use support::*;

#[test]
fn missing_facts_fail_without_manufacturing_a_live_process() {
    let metadata = snapshot("linux");
    assert!(matches!(
        manager_snapshot(&metadata, &ManagerFacts::default()),
        Err(AdapterError::CapabilityUnavailable {
            capability: "manager settings"
        })
    ));
    let mut facts = facts("linux");
    facts.sessions.clear();
    assert!(matches!(
        manager_snapshot(&metadata, &facts),
        Err(AdapterError::CapabilityUnavailable { .. })
    ));
    let mut session_facts = session_facts();
    session_facts.tabs.clear();
    assert!(matches!(
        validate_session_facts(&metadata.sessions[0], &session_facts),
        Err(AdapterError::CapabilityUnavailable {
            capability: "tab runtime facts"
        })
    ));
    let dto = manager_snapshot(&metadata, &support::facts("linux")).unwrap();
    assert!(matches!(dto.sessions[0].status, wire::Status::Unknown));
    assert!(matches!(
        dto.sessions[0].tabs[0].status,
        wire::TabStatus::Unknown
    ));
    assert_eq!(dto.sessions[0].counts.running, 0);
    assert!(!dto.sessions[0].can_add_tab);
    assert!(!dto.probe.available);
}

#[test]
fn session_projection_does_not_need_global_manager_facts() {
    let metadata = snapshot("linux");
    let dto = session_dto(&metadata.sessions[0], &session_facts()).unwrap();
    assert_eq!(dto.tabs[0].created_at, "2026-07-07T12:00:01.000Z");
    assert_ne!(dto.tabs[0].created_at, dto.created_at);
    assert_eq!(dto.env, metadata.sessions[0].env);
}

#[test]
fn settings_preserve_host_fields_and_project_only_shared_preferences() {
    let metadata = snapshot("linux");
    let settings = settings_dto(
        &metadata.preferences,
        facts("linux").settings.as_ref().unwrap(),
    )
    .unwrap();
    assert_eq!(settings.accent_color, "#123456");
    assert_eq!(settings.background_color, "#654321");
    assert_eq!(settings.history_page_size, 17);
    assert_eq!(settings.session_settle_hours, 48.0);
    assert_eq!(settings.session_delete_hours, 1440.0);
    assert!(!settings.auto_update);
    assert_eq!(settings.process_rules[0].id, "custom-agent-1");
    assert_eq!(settings.terminal_env[0].name, "GLOBAL");
    assert_eq!(
        settings.terminal_profile_id,
        Some(Some("login-shell".into()))
    );
    let value = serde_json::to_value(settings).unwrap();
    assert!(value.get("fontFamily").is_none());
    assert!(value.get("fontSize").is_none());
}

#[test]
fn statuses_and_counts_match_shared_status_rules() {
    let mut metadata = snapshot("linux");
    let mut facts = session_facts();
    let id = TAB.parse().unwrap();
    let tab = facts.tabs.get_mut(&id).unwrap();
    tab.lifecycle = wire::TabLifecycle::Open;
    tab.observation = ObservationFacts::Alive {
        agents: 0,
        healthy: true,
        reason: None,
    };
    let dto = session_dto(&metadata.sessions[0], &facts).unwrap();
    assert!(matches!(dto.status, wire::Status::Waiting));
    assert_eq!(dto.counts.waiting, 1);
    facts.tabs.get_mut(&id).unwrap().observation = ObservationFacts::Alive {
        agents: 2,
        healthy: true,
        reason: None,
    };
    let dto = session_dto(&metadata.sessions[0], &facts).unwrap();
    assert!(matches!(dto.status, wire::Status::Running));
    assert_eq!(dto.counts.running, 1);
    assert_eq!(dto.counts.agents, 2);
    metadata.sessions[0].settled_at = Some(TIME.into());
    let dto = session_dto(&metadata.sessions[0], &facts).unwrap();
    assert!(matches!(dto.status, wire::Status::Settled));
    assert!(matches!(dto.activity_status, wire::TabStatus::Running));
    let tab = facts.tabs.get_mut(&id).unwrap();
    tab.lifecycle = wire::TabLifecycle::Closed;
    tab.observation = ObservationFacts::Exited;
    tab.error = Some(wire::AppError {
        code: wire::ErrorCode::LaunchFailed,
        message: "Failed".into(),
        retryable: false,
    });
    let dto = session_dto(&metadata.sessions[0], &facts).unwrap();
    assert_eq!(dto.counts.closed, 1);
    assert_eq!(dto.counts.error, 1);
    assert_eq!(dto.counts.agents, 0);
    assert!(matches!(dto.activity_status, wire::TabStatus::Error));
}

#[test]
fn reject_invalid_metadata_and_contradictory_runtime_evidence() {
    let mut metadata = snapshot("linux");
    let mut facts = session_facts();
    let id = TAB.parse().unwrap();
    facts.tabs.get_mut(&id).unwrap().observation = ObservationFacts::Alive {
        agents: 1,
        healthy: true,
        reason: None,
    };
    assert!(matches!(
        session_dto(&metadata.sessions[0], &facts),
        Err(AdapterError::InvalidFacts { .. })
    ));
    facts.tabs.get_mut(&id).unwrap().lifecycle = wire::TabLifecycle::Open;
    metadata.sessions[0].tabs[0].state = core::TabState::Archived;
    assert!(session_dto(&metadata.sessions[0], &facts).is_err());
    metadata.sessions[0].tabs[0].state = core::TabState::Pending;
    metadata.sessions[0].tabs[0].session_id = core::SessionId::new();
    assert!(session_dto(&metadata.sessions[0], &facts).is_err());
    let mut metadata = snapshot("linux");
    metadata.sessions.push(metadata.sessions[0].clone());
    assert!(manager_snapshot(&metadata, &support::facts("linux")).is_err());
}

#[test]
fn validate_every_dto_before_returning_it() {
    let metadata = snapshot("linux");
    let mut facts = facts("linux");
    facts.probe.as_mut().unwrap().capabilities.split_pane = true;
    assert!(manager_snapshot(&metadata, &facts).is_err());
    let mut facts = session_facts();
    facts
        .tabs
        .get_mut(&TAB.parse().unwrap())
        .unwrap()
        .created_at = "not a timestamp".into();
    assert!(session_dto(&metadata.sessions[0], &facts).is_err());
    for invalid in [
        "2026-07-07t12:00:00Z",
        "2026-07-07 12:00:00Z",
        "2026-07-07T12:00:60Z",
    ] {
        let mut facts = session_facts();
        facts
            .tabs
            .get_mut(&TAB.parse().unwrap())
            .unwrap()
            .created_at = invalid.into();
        assert!(session_dto(&metadata.sessions[0], &facts).is_err());
    }
    let mut metadata = snapshot("linux");
    metadata.sessions[0].id = "11111111-1111-0111-0111-111111111111".parse().unwrap();
    metadata.sessions[0].tabs[0].session_id = metadata.sessions[0].id;
    assert!(session_dto(&metadata.sessions[0], &session_facts()).is_err());
}

#[test]
fn attachments_are_fenced_and_never_claim_partial_or_binary_recovery() {
    let metadata = snapshot("linux");
    let id = TAB.parse().unwrap();
    assert!(matches!(
        terminal_attachment(&metadata, id, None),
        Err(AdapterError::CapabilityUnavailable { .. })
    ));
    let mut facts = attachment();
    let dto = terminal_attachment(&metadata, id, Some(&facts)).unwrap();
    let wire::TerminalAttachment::Owned(dto) = dto else {
        panic!("expected owned");
    };
    assert_eq!(dto.control_epoch, 3);
    assert_eq!(dto.chunks[0].generation.to_string(), GENERATION);
    assert_eq!(dto.settled_at, Some(None));
    for recovery in [
        RecoveryCapability::VisibleScreen,
        RecoveryCapability::FormattedActiveBufferV1,
        RecoveryCapability::GhosttyBinary,
    ] {
        let AttachmentDelivery::Owned(replay) = &mut facts.delivery else {
            unreachable!()
        };
        replay.recovery = recovery;
        assert!(matches!(
            terminal_attachment(&metadata, id, Some(&facts)),
            Err(AdapterError::CapabilityUnavailable { .. })
        ));
    }
    facts.delivery = AttachmentDelivery::Blocked;
    assert!(matches!(
        terminal_attachment(&metadata, id, Some(&facts)).unwrap(),
        wire::TerminalAttachment::Blocked { .. }
    ));
    facts.control_epoch = 0;
    assert!(terminal_attachment(&metadata, id, Some(&facts)).is_err());
}

#[test]
fn attachments_reject_reordered_oversize_and_out_of_range_chunks() {
    let metadata = snapshot("linux");
    let id = TAB.parse().unwrap();
    for case in 0..5 {
        let mut facts = attachment();
        let AttachmentDelivery::Owned(replay) = &mut facts.delivery else {
            unreachable!()
        };
        match case {
            0 => replay.chunks.swap(0, 1),
            1 => replay.chunks[0].sequence = 9,
            2 => replay.chunks[0].data = "世".repeat(6000),
            3 => replay.cols = 1,
            _ => replay.last_sequence = wire::MAX_SAFE_INTEGER + 1,
        }
        assert!(
            terminal_attachment(&metadata, id, Some(&facts)).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn event_mapping_uses_explicit_exit_facts_and_current_settlement() {
    let mut metadata = snapshot("linux");
    metadata.sessions[0].settled_at = Some(TIME.into());
    let id = TAB.parse().unwrap();
    for event in events() {
        let dto = terminal_event(&metadata, id, Some(&event)).unwrap();
        wire::Validate::validate(&dto).unwrap();
        if let wire::HostEvent::Terminal {
            event: wire::TerminalEvent::Exit {
                exit_code, signal, ..
            },
        } = dto
        {
            assert_eq!(exit_code, None);
            assert_eq!(signal, None);
        }
    }
    let event = TerminalEventFacts::Settlement {
        generation: GENERATION.parse().unwrap(),
    };
    let dto = terminal_event(&metadata, id, Some(&event)).unwrap();
    assert_eq!(
        serde_json::to_value(dto).unwrap()["event"]["settledAt"],
        TIME
    );
    assert!(terminal_event(&metadata, id, None).is_err());
    assert!(terminal_event(&metadata, core::TabId::new(), Some(&event)).is_err());
    let invalid = TerminalEventFacts::Data {
        generation: GENERATION.parse().unwrap(),
        control_epoch: 0,
        sequence: 1,
        data: "x".into(),
    };
    assert!(terminal_event(&metadata, id, Some(&invalid)).is_err());
}

#[test]
fn reject_unsafe_signed_integers_and_non_wire_uuid_variants() {
    let metadata = snapshot("linux");
    let id = TAB.parse().unwrap();
    let mut facts = session_facts();
    facts.tabs.get_mut(&id).unwrap().exit_code = Some(Some(i64::MAX));
    assert!(session_dto(&metadata.sessions[0], &facts).is_err());
    for value in [i64::MAX, i64::MIN] {
        let event = TerminalEventFacts::Exit {
            generation: GENERATION.parse().unwrap(),
            control_epoch: 1,
            exit_code: Some(value),
            signal: None,
            last_sequence: 0,
        };
        assert!(terminal_event(&metadata, id, Some(&event)).is_err());
    }
    let invalid_id = "22222222-2222-0222-0222-222222222222";
    let mut metadata = metadata;
    metadata.sessions[0].tabs[0].id = invalid_id.parse().unwrap();
    assert!(terminal_event(
        &metadata,
        invalid_id.parse().unwrap(),
        Some(&TerminalEventFacts::Activity { busy: true })
    )
    .is_err());
    assert!(changed_event(&core::ChangedEvent {
        revision: 1,
        reason: core::ChangeReason::Sessions,
        select_session_id: Some(invalid_id.parse().unwrap()),
    })
    .is_err());
}

#[test]
fn unavailable_maps_to_explicit_unsupported_error() {
    let error: wire::AppError = AdapterError::CapabilityUnavailable {
        capability: "process observation",
    }
    .into();
    assert_eq!(error.code, wire::ErrorCode::Unsupported);
    assert!(!error.retryable);
    assert!(error.message.contains("process observation"));
}

#[test]
fn emit_zod_fixtures() {
    let fixtures = fixture_document();
    assert_eq!(fixtures["snapshots"].as_array().unwrap().len(), 6);
    if let Some(path) = std::env::var_os("SHELLFOX_ADAPTER_FIXTURES") {
        std::fs::write(path, serde_json::to_vec_pretty(&fixtures).unwrap()).unwrap();
    }
}
