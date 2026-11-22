//! The consumer is woken for queued events instead of polling try_recv.
#[path = "support/external.rs"]
mod common;
use common::*;
use shellfox_runtime::*;
use std::{
    sync::{mpsc, Arc},
    time::Duration,
};

fn next_event(runtime: &Runtime, woken: &mpsc::Receiver<()>) -> Event {
    loop {
        if let Ok(event) = runtime.try_recv() {
            return event;
        }
        woken
            .recv_timeout(Duration::from_secs(5))
            .expect("queued events wake the consumer");
    }
}

#[test]
fn event_wake_fires_on_registration_and_for_each_burst_of_events() {
    let (runtime, mut source) = source();
    let observer = observe(&runtime, 1, 11, Some(0));
    while runtime.try_recv().is_ok() {}
    let (wake, woken) = mpsc::channel();
    runtime.set_event_wake(Arc::new(move || {
        let _ = wake.send(());
    }));
    woken
        .recv_timeout(Duration::from_secs(1))
        .expect("registration wakes once for events queued earlier");
    feed(&mut source, 1, ExternalStream::Stdout, b"hello".to_vec());
    loop {
        match next_event(&runtime, &woken) {
            Event::ObserverOutput {
                observer: got,
                chunk,
            } => {
                assert_eq!(got, observer);
                assert_eq!(chunk.sequence, 1);
                break;
            }
            Event::Phase { .. } => {}
            event => panic!("unexpected event: {event:?}"),
        }
    }
    finish(&mut source, 1);
    loop {
        if let Event::ObserverExited { observer: got, .. } = next_event(&runtime, &woken) {
            assert_eq!(got, observer);
            break;
        }
    }
}
