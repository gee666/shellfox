use super::*;
use catalog::{id, known, unknown};
use v2::{Fact, UnknownReason};
pub(super) fn project(service: &Service, tab: &shellfox_core::Tab) -> Result<v2::Tab, AppError> {
    let terminal = service
        .terminals
        .get(&tab.id)
        .filter(|terminal| terminal.runtime.identity().session_id == tab.session_id);
    let mut facts = v2::TabFacts {
        cwd: known(tab.cwd.clone()),
        ordinal: known(u64::from(tab.ordinal)),
        created_at: tab.created_at.clone().map(known).unwrap_or(Fact::Unknown {
            reason: UnknownReason::NotRecorded,
        }),
        lifecycle: unknown(),
        status: known(wire::TabStatus::Unknown),
        agents: unknown(),
        monitoring_reason: known(Some(
            "No current owned process evidence is available.".into(),
        )),
        error: unknown(),
        generation: known(None),
        terminal_kind: unknown(),
        profile_id: known(Some(tab.profile_id.to_string())),
        exit_code: unknown(),
    };
    let mut source_kind = unknown();
    if let Some(terminal) = terminal {
        let runtime = &terminal.runtime;
        let evidence = crate::facts_capture::terminal(&service.activity.owner, terminal);
        source_kind = crate::remote_v2::source_kind(Some(&evidence));
        facts.generation = known(Some(id(runtime.identity().generation)?));
        if runtime.source_kind() == shellfox_runtime::SourceKind::OwnedPty {
            facts.terminal_kind = known(Some(wire::TerminalKind::Embedded));
            use crate::activity_integration::StartupEvidence as Start;
            use shellfox_runtime::Phase;
            facts.lifecycle = match (&evidence.startup, runtime.phase()) {
                (Start::Confirmed, Phase::Active) => known(wire::TabLifecycle::Open),
                (_, Phase::Closed) => known(wire::TabLifecycle::Closed),
                (Start::Pending, Phase::Starting) => known(wire::TabLifecycle::Launching),
                (Start::Failed { .. }, _) => known(wire::TabLifecycle::LaunchUncertain),
                _ => unknown(),
            };
            if let Start::Failed { sanitized_error, acknowledged: false } = &evidence.startup {
                facts.error = known(Some(sanitized_error.clone()));
                facts.status = known(wire::TabStatus::Error);
            } else if matches!(evidence.startup, Start::Confirmed | Start::Failed { acknowledged: true, .. }) {
                facts.error = known(None);
            }
            if let Some(observation) = &evidence.observation {
                use crate::activity::{Health, RootState};
                facts.monitoring_reason = known(observation.reason.clone());
                if observation.root == RootState::Alive {
                    facts.agents = known(u64::from(observation.agents));
                    if matches!(facts.lifecycle.known(), Some(wire::TabLifecycle::Open)) {
                        facts.status = known(if observation.agents > 0 {
                            wire::TabStatus::Running
                        } else if observation.health == Health::Healthy
                            && evidence.activity_available
                        {
                            wire::TabStatus::Waiting
                        } else {
                            wire::TabStatus::Unknown
                        });
                    }
                } else if observation.root == RootState::Exited {
                    facts.agents = known(0);
                }
            }
        } else {
            facts.terminal_kind = Fact::NotApplicable;
            // Parser Finish alone is not process evidence. Only the matched,
            // published run can confirm its process exited and output ended.
            if let Some(code) = service.completed_automation_exit_code(terminal) {
                facts.lifecycle = known(wire::TabLifecycle::Closed);
                facts.exit_code = known(Some(code));
                facts.monitoring_reason = known(Some("Completed automation replay.".into()));
            } else {
                facts.monitoring_reason = known(Some(
                    "External presentation does not prove raw process state or cleanup.".into(),
                ));
            }
        }
    } else if tab.state == shellfox_core::TabState::Archived {
        facts.lifecycle = known(wire::TabLifecycle::Closed);
    }
    Ok(v2::Tab {
        id: id(tab.id)?,
        session_id: id(tab.session_id)?,
        title: tab.title.clone(),
        source_kind,
        facts,
    })
}
