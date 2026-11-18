use super::*;

fn report(activity: ActivityShutdownState) -> anyhow::Result<Option<ShutdownOutcome>> {
    Ok(Some(ShutdownOutcome {
        activity,
        presentation: shellfox_service::PresentationShutdownState::NoKnownFailure,
    }))
}

#[test]
fn peer_mutation_recovery_or_input_pending_blocks_close_without_a_local_operation() {
    assert_eq!(
        host_operation_guard(false, true),
        WindowCloseGuard::HostOperation
    );
    assert_eq!(
        host_operation_guard(true, false),
        WindowCloseGuard::HostOperation
    );
    assert_eq!(host_operation_guard(false, false), WindowCloseGuard::Ready);
}
#[test]
fn dirty_report_preserves_exact_counts_without_claiming_commit_or_live_resources() {
    for count in [0, 7, usize::MAX] {
        let mut output = Vec::new();
        log_shutdown_report(
            &mut output,
            report(ActivityShutdownState::DirtyAfterOwnershipLoss {
                pending_sessions: count,
            }),
        );
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!(
                "Final host close: resources closed; {count} session activity updates were not committed after ownership loss.\n"
            )
        );
    }
}

#[test]
fn absent_or_unreadable_report_remains_unknown() {
    let mut output = Vec::new();
    log_shutdown_report(&mut output, Ok(None));
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "Final host close: activity and presentation status are unknown; no shutdown report was available.\n"
    );
    let mut output = Vec::new();
    log_shutdown_report(&mut output, Err(anyhow::anyhow!("registry unavailable")));
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "Final host close: shutdown report could not be read; activity and presentation status are unknown: registry unavailable\n"
    );
}

#[test]
fn only_committed_report_logs_a_successful_commit() {
    let mut output = Vec::new();
    log_shutdown_report(&mut output, report(ActivityShutdownState::Committed));
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "Final host close: session activity updates committed.\n"
    );
}

#[test]
fn presentation_warning_format_reports_count_without_raw_evidence_or_completion_claim() {
    // Formatting only. No sealed failed-source proof is fabricated by this consumer test.
    for count in [1, 64] {
        let mut output = Vec::new();
        log_presentation_failures(&mut output, count);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!(
                "Warning: host resources are confirmed closed; final presentation failed for {count} source(s). Opaque failure evidence remains retained.\n"
            )
        );
    }
}

#[test]
fn unavailable_log_sink_does_not_change_close_result() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    log_shutdown_report(
        &mut Broken,
        report(ActivityShutdownState::DirtyAfterOwnershipLoss {
            pending_sessions: 3,
        }),
    );
}
