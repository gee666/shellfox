use super::{delivery::ObserverDelivery, identity, request};
use shellfox_runtime::*;
use std::time::{Duration, Instant};

#[test]
fn actual_formatted_recovery_keeps_history_and_unfinished_utf8_continuation() {
    let mut runtime = Runtime::start_external(identity(), ExternalConfig::default()).unwrap();
    let mut source = runtime.take_source().unwrap();
    source
        .try_feed(ExternalChunk {
            sequence: SourceSequence(1),
            stream: ExternalStream::Stdout,
            bytes: format!("history-start\r\n{}", "scroll-line\r\n".repeat(40)).into_bytes(),
        })
        .unwrap();
    source
        .try_feed(ExternalChunk {
            sequence: SourceSequence(2),
            stream: ExternalStream::Stdout,
            bytes: vec![0xe2, 0x82],
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while runtime.external_status().unwrap().source_sequence != SourceSequence(2) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut view = None;
    let Response::Observed(observer) = request(
        &runtime,
        1,
        Command::Observe {
            viewer: 1,
            after: None,
        },
        &mut view,
    )
    .unwrap() else {
        panic!("observer")
    };
    view = Some(ObserverDelivery::new(runtime.identity(), observer).unwrap());
    let response = request(
        &runtime,
        2,
        Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
        &mut view,
    )
    .unwrap();
    let restoration = view.as_mut().unwrap().recover(response).unwrap();
    let bytes = restoration.bytes().unwrap();
    assert!(bytes
        .windows(b"history-start".len())
        .any(|text| text == b"history-start"));
    assert!(
        bytes.ends_with(&[0xe2, 0x82]),
        "the exact continuation stays last, before subsequent live output"
    );
    assert_eq!(
        restoration.last_sequence,
        runtime.external_status().unwrap().last_delivery_sequence.0
    );
    let response = request(&runtime, 3, Command::Unobserve { observer }, &mut view).unwrap();
    assert!(matches!(response, Response::Applied));
}
