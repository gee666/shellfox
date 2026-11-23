use super::*;
#[test]
fn files_reports_initialization_failure_instead_of_missing_backend_defaults() {
    let outcome = EditorOutcome::Unavailable(
        crate::editor_composition::EditorUnavailableReason::PreparationFailed(
            "Files unavailable: fixture initialization refused.".into(),
        ),
    );
    assert_eq!(
        files_unavailable(Some(&outcome)),
        "Files unavailable: fixture initialization refused."
    );
    assert_ne!(files_unavailable(None), files_unavailable(Some(&outcome)));
}
#[test]
fn file_edit_during_async_terminal_close_blocks_final_removal() {
    assert_eq!(close_guard(false, false), CloseGuard::Ready);
    assert_eq!(close_guard(true, false), CloseGuard::Dirty);
}
#[test]
fn in_flight_write_blocks_close_even_if_buffer_matches_old_saved_text() {
    assert_eq!(close_guard(false, true), CloseGuard::Pending);
    assert_eq!(close_guard(true, true), CloseGuard::Pending);
    assert_eq!(close_guard(true, false), CloseGuard::Dirty);
}
#[test]
fn ui_source_switch_roundtrip_cannot_publish_local_or_foreign_results() {
    assert!(same_ui_source(1, 1, true, true));
    assert!(!same_ui_source(1, 2, true, false));
    assert!(!same_ui_source(1, 3, true, true));
    assert!(!same_ui_source(2, 2, false, true));
    assert!(same_ui_source(2, 2, false, false));
}
