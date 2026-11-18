use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
pub(super) fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
pub(super) fn source() -> (Runtime, ExternalSource) {
    source_with_policy(ExternalDisplayPolicy::RawBytes)
}
pub(super) fn source_with_policy(policy: ExternalDisplayPolicy) -> (Runtime, ExternalSource) {
    let mut runtime =
        Runtime::start_external_with_policy(identity(), ExternalConfig::default(), policy).unwrap();
    assert_eq!(runtime.source_kind(), SourceKind::ExternalBytes);
    assert_eq!(runtime.phase(), Phase::Active);
    let source = runtime.take_source().expect("one producer");
    assert!(runtime.take_source().is_none());
    (runtime, source)
}
pub(super) fn feed(
    source: &mut ExternalSource,
    sequence: u64,
    stream: ExternalStream,
    bytes: Vec<u8>,
) {
    let mut chunk = ExternalChunk {
        sequence: SourceSequence(sequence),
        stream,
        bytes,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match source.try_feed(chunk) {
            Ok(()) => return,
            Err(ExternalFeedError::Full(returned)) => {
                assert!(Instant::now() < deadline);
                chunk = returned;
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("external fixture feed failed: {error:?}"),
        }
    }
}
pub(super) fn finish(source: &mut ExternalSource, last: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match source.try_finish(SourceSequence(last)) {
            Ok(()) => return,
            Err(ExternalFinishError::Full) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("external fixture finish failed: {error:?}"),
        }
    }
}
pub(super) fn wait(runtime: &Runtime, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(Instant::now() < deadline);
        assert!(!runtime.external_status().unwrap().failed);
        std::thread::sleep(Duration::from_millis(1));
    }
}
pub(super) fn request(
    runtime: &Runtime,
    id: u64,
    command: Command,
) -> Result<Response, RuntimeError> {
    runtime
        .try_send(Request {
            id: RequestId(id),
            command,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        match runtime.try_recv() {
            Ok(Event::Reply { id: got, result }) if got == RequestId(id) => return result,
            Ok(Event::Started(Err(error))) => panic!("external start failed: {error}"),
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}
pub(super) fn observe(
    runtime: &Runtime,
    id: u64,
    viewer: u64,
    after: Option<u64>,
) -> ObserverLease {
    match request(runtime, id, Command::Observe { viewer, after }).unwrap() {
        Response::Observed(lease) => lease,
        _ => panic!("observer expected"),
    }
}
pub(super) fn frame(runtime: &Runtime, id: u64, observer: ObserverLease) -> Arc<ViewFrame> {
    match request(
        runtime,
        id,
        Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::Frame { focused: true },
        },
    )
    .unwrap()
    {
        Response::Presentation(frame) => frame,
        _ => panic!("frame expected"),
    }
}
pub(super) fn text(frame: &ViewFrame) -> String {
    frame
        .screen
        .screen
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect()
}
