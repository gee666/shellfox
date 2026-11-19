use super::*;
use crate::{
    activity::{Health, RootState},
    activity_integration::{StartupEvidence, TerminalSourceFacts},
};
use shellfox_remote::adapter::{ObservationFacts, TabFacts};
use shellfox_runtime::Phase;

pub(super) fn project(
    tab: &shellfox_core::Tab,
    runtime: Option<&TerminalSourceFacts>,
) -> Result<TabFacts, AppError> {
    let created_at = tab.created_at.clone().ok_or_else(|| {
        incompatible("V1 requires the recorded tab creation time; session/poll timestamps are not substitutes.")
    })?;
    let (lifecycle, observation, generation, terminal_kind, error) = match runtime {
        Some(runtime) => {
            if runtime.identity.tab_id != tab.id || runtime.identity.session_id != tab.session_id {
                return Err(unavailable(
                    "Owned runtime identity does not match tab membership.",
                ));
            }
            if runtime.source_kind == Some(shellfox_runtime::SourceKind::ExternalBytes) {
                return Err(incompatible("V1 cannot represent a read-only external byte source. Use observer-v2."));
            }
            let (lifecycle, observation, error) = runtime_state(runtime)?;
            (
                lifecycle,
                observation,
                Some(runtime.identity.generation),
                Some(wire::TerminalKind::Embedded),
                error,
            )
        }
        None => (
            if tab.state == shellfox_core::TabState::Archived {
                wire::TabLifecycle::Closed
            } else {
                wire::TabLifecycle::LaunchUncertain
            },
            missing("No current owned runtime generation is registered for this tab."),
            None,
            None,
            None,
        ),
    };
    if tab.state == shellfox_core::TabState::Archived
        && !matches!(lifecycle, wire::TabLifecycle::Closed)
    {
        return Err(unavailable(
            "Archived metadata conflicts with a current owned runtime; refresh owner state.",
        ));
    }
    Ok(TabFacts {
        created_at,
        lifecycle,
        observation,
        error,
        generation,
        terminal_kind,
        exit_code: None,
    })
}
fn runtime_state(
    runtime: &TerminalSourceFacts,
) -> Result<(wire::TabLifecycle, ObservationFacts, Option<AppError>), AppError> {
    use wire::TabLifecycle as Life;
    match &runtime.startup {
        StartupEvidence::Pending => match runtime.phase {
            Phase::Starting => Ok((
                Life::Launching,
                missing("Owned startup acknowledgement is pending."),
                None,
            )),
            Phase::Closed => Ok((
                Life::Closed,
                missing("Owned resources closed before startup evidence was captured."),
                None,
            )),
            _ => Err(unavailable(
                "Startup outcome is pending; phase alone cannot prove launch success or failure.",
            )),
        },
        StartupEvidence::Failed { sanitized_error, acknowledged } => {
            if runtime.phase == Phase::Active {
                return Err(unavailable(
                    "Active phase contradicts actual failed-start evidence.",
                ));
            }
            Ok((
                if runtime.phase == Phase::Closed {
                    Life::Closed
                } else {
                    Life::LaunchUncertain
                },
                missing(&sanitized_error.message),
                (!acknowledged).then(|| sanitized_error.clone()),
            ))
        }
        StartupEvidence::Confirmed => match runtime.phase {
            Phase::Starting => Err(unavailable(
                "Confirmed startup has not reached a consistent owner phase.",
            )),
            Phase::Active => Ok((Life::Open, observation(runtime), None)),
            Phase::Closed => Ok((Life::Closed, ObservationFacts::Exited, None)),
            Phase::Draining => {
                // Electron terminal/service.ts231-240: root exit closes the
                // terminal lifecycle, not descendants/ownership cleanup proof.
                let exited = runtime
                    .observation
                    .as_ref()
                    .is_some_and(|value| value.root == RootState::Exited);
                Ok((
                    if exited { Life::Closed } else { Life::Open },
                    missing(if exited {
                        "Owned root exited; descendant cleanup remains pending."
                    } else {
                        "Owned terminal cleanup remains pending; root exit is not confirmed."
                    }),
                    None,
                ))
            }
        },
    }
}
#[cfg(test)]
mod acknowledgement_tests {
    use super::*;

    #[test]
    fn clearing_a_failed_launch_hides_error_without_claiming_start_or_cleanup() {
        let mut facts = TerminalSourceFacts {
            identity: shellfox_runtime::Identity { session_id: Default::default(), tab_id: Default::default(), generation: Default::default() },
            phase: Phase::Draining,
            source_kind: None,
            startup: StartupEvidence::Failed {
                sanitized_error: AppError { code: wire::ErrorCode::LaunchFailed, message: "Owned terminal startup failed.".into(), retryable: true },
                acknowledged: false,
            },
            observation: None,
            busy: false,
            activity_available: false,
        };
        let (phase, _, before) = runtime_state(&facts).unwrap();
        assert!(matches!(phase, wire::TabLifecycle::LaunchUncertain));
        assert!(before.is_some());
        if let StartupEvidence::Failed { acknowledged, .. } = &mut facts.startup { *acknowledged = true; }
        let (phase, _, after) = runtime_state(&facts).unwrap();
        assert!(matches!(phase, wire::TabLifecycle::LaunchUncertain));
        assert!(after.is_none());
    }
}

fn observation(runtime: &TerminalSourceFacts) -> ObservationFacts {
    match &runtime.observation {
        Some(value) if value.root == RootState::Alive => ObservationFacts::Alive {
            agents: u64::from(value.agents),
            healthy: value.health == Health::Healthy && runtime.activity_available,
            reason: if runtime.activity_available {
                value.reason.clone()
            } else {
                Some("Authoritative output activity reporting is unavailable.".into())
            },
        },
        Some(value) if value.root == RootState::Exited => ObservationFacts::Exited,
        Some(value) => missing(
            value
                .reason
                .as_deref()
                .unwrap_or("Owned process evidence is unavailable."),
        ),
        None => missing("Owned process inspection has not produced evidence for this generation."),
    }
}
fn missing(reason: &str) -> ObservationFacts {
    ObservationFacts::Unavailable {
        reason: reason.into(),
    }
}
