use super::*;

#[test]
fn authoritative_feed_full_preserves_exact_event_without_sequence_remap() {
    let (source, publisher) = AutomationView::new();
    for sequence in 1..=32 {
        publisher
            .try_push(AutomationViewEvent::Data {
                sequence,
                stream: AutomationStream::Stdout,
                bytes: vec![sequence as u8; 32768],
            })
            .unwrap();
    }
    let pending = publisher
        .try_push(AutomationViewEvent::Data {
            sequence: 33,
            stream: AutomationStream::Stderr,
            bytes: vec![255; 32768],
        })
        .unwrap_err();
    assert!(matches!(
        source.try_recv(),
        Some(AutomationViewEvent::Data { sequence: 1, .. })
    ));
    publisher.try_push(pending).unwrap();
    for sequence in 2..=33 {
        match source.try_recv().unwrap() {
            AutomationViewEvent::Data {
                sequence: actual,
                stream,
                bytes,
            } => {
                assert_eq!(actual, sequence);
                if sequence == 33 {
                    assert_eq!(stream, AutomationStream::Stderr);
                    assert_eq!(bytes, vec![255; 32768]);
                }
            }
            _ => panic!("Unexpected non-data event"),
        }
    }
    assert!(source.try_recv().is_none());
}
#[test]
fn authoritative_eofs_and_exit_follow_final_data() {
    let (source, publisher) = AutomationView::new();
    publisher
        .try_push(AutomationViewEvent::Data {
            sequence: 1,
            stream: AutomationStream::Stdout,
            bytes: vec![0, 255],
        })
        .unwrap();
    publisher
        .try_push(AutomationViewEvent::Eof {
            after_sequence: 1,
            stream: AutomationStream::Stdout,
        })
        .unwrap();
    publisher
        .try_push(AutomationViewEvent::Eof {
            after_sequence: 1,
            stream: AutomationStream::Stderr,
        })
        .unwrap();
    publisher
        .try_push(AutomationViewEvent::Exit {
            after_sequence: 1,
            code: 7,
        })
        .unwrap();
    assert!(matches!(
        source.try_recv(),
        Some(AutomationViewEvent::Data { sequence: 1, .. })
    ));
    assert!(matches!(
        source.try_recv(),
        Some(AutomationViewEvent::Eof {
            after_sequence: 1,
            stream: AutomationStream::Stdout
        })
    ));
    assert!(matches!(
        source.try_recv(),
        Some(AutomationViewEvent::Eof {
            after_sequence: 1,
            stream: AutomationStream::Stderr
        })
    ));
    assert!(matches!(
        source.try_recv(),
        Some(AutomationViewEvent::Exit {
            after_sequence: 1,
            code: 7
        })
    ));
    drop(source);
    assert!(!publisher.alive());
}
#[path = "automation_display_tests.rs"]
mod display;
#[cfg(windows)]
#[path = "automation_launch_tests.rs"]
mod launch;
#[cfg(windows)]
#[path = "automation_native_tests.rs"]
mod native;
#[path = "automation_relay_tests.rs"]
mod relay;
#[path = "automation_retirement_tests.rs"]
mod retirement;
