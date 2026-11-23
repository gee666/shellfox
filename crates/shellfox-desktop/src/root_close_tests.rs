use super::*;

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
