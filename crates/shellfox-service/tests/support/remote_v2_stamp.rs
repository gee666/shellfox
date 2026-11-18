use super::{delivery::ObserverDelivery, request};
use shellfox_runtime::*;

/// Policy/codec fixtures obtain opaque IDs from a real isolated Runtime. No
/// test constructs RecoveryId or invents its identity/sequence/layout binding.
pub(super) fn recovery(view: &ObserverDelivery) -> Response {
    let runtime = Runtime::start_external(view.identity(), ExternalConfig::default()).unwrap();
    let mut unused = None;
    let Response::Observed(observer) = request(
        &runtime,
        1,
        Command::Observe {
            viewer: view.observer().viewer,
            after: None,
        },
        &mut unused,
    )
    .unwrap() else {
        panic!("observer")
    };
    assert_eq!(observer, view.observer());
    request(
        &runtime,
        2,
        Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
        &mut unused,
    )
    .unwrap()
}
