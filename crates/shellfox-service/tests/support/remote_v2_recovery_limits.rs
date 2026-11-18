use super::{
    delivery::{DeliveryFailure, ObserverDelivery},
    identity, recovery,
};
use shellfox_protocol::observer_v2 as wire;
use shellfox_runtime::*;

#[test]
fn full_recovery_bound_uses_the_canonical_codec_without_interior_padding_or_truncation() {
    let mut view = ObserverDelivery::new(
        identity(),
        ObserverLease {
            viewer: 1,
            epoch: 1,
        },
    )
    .unwrap();
    let mut payload = vec![0x80; wire::MAX_RECOVERY_BYTES];
    payload[..3].copy_from_slice(b"\x18\x1bc");
    let Response::Recovery(mut stamp) = recovery(&view) else {
        panic!("stamped recovery")
    };
    stamp.bytes = payload.clone();
    let encoded = view.recover(Response::Recovery(stamp)).unwrap();
    assert_eq!(encoded.bytes().unwrap(), payload);
    let Response::Recovery(mut stamp) = recovery(&view) else {
        panic!("stamped recovery")
    };
    stamp.bytes = vec![0; wire::MAX_RECOVERY_BYTES + 1];
    assert_eq!(
        view.recover(Response::Recovery(stamp)).unwrap_err(),
        DeliveryFailure::RecoveryUnavailable
    );
    let Response::Recovery(mut stamp) = recovery(&view) else {
        panic!("stamped recovery")
    };
    stamp.cols = 1;
    assert_eq!(
        view.recover(Response::Recovery(stamp)).unwrap_err(),
        DeliveryFailure::GeometryUnsupported
    );
}
