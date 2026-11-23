use super::*;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::{
    Command, Event, ExternalConfig, ExternalDisplayPolicy, Identity, Response, Runtime,
    SourceSequence,
};
use AutomationStream::{Stderr, Stdout};
enum Input {
    Data(AutomationStream, &'static [u8]),
    Eof(AutomationStream),
}
struct Case {
    name: &'static str,
    input: Vec<Input>,
    display: Vec<&'static str>,
    zero_before_finish: bool,
}
fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "split stream UTF8",
            input: vec![
                Input::Data(Stdout, &[226, 130]),
                Input::Data(Stderr, &[172]),
                Input::Data(Stdout, &[172, 10]),
            ],
            display: vec!["�", "€\r\n"],
            zero_before_finish: false,
        },
        Case {
            name: "stdout EOF before stderr final data",
            input: vec![
                Input::Data(Stdout, &[240, 159]),
                Input::Eof(Stdout),
                Input::Data(Stderr, b"END\n"),
                Input::Eof(Stderr),
            ],
            display: vec!["END\r\n", "�"],
            zero_before_finish: false,
        },
        Case {
            name: "both incomplete until combined close flush",
            input: vec![
                Input::Data(Stderr, &[226, 130]),
                Input::Data(Stdout, &[240, 159]),
                Input::Eof(Stderr),
                Input::Eof(Stdout),
            ],
            display: vec!["��"],
            zero_before_finish: true,
        },
        Case {
            name: "CRLF split and interleaved callbacks",
            input: vec![
                Input::Data(Stdout, b"A\r"),
                Input::Data(Stderr, b"\n"),
                Input::Data(Stdout, b"\r\n"),
                Input::Data(Stdout, b"\r"),
                Input::Data(Stdout, b"\n"),
            ],
            display: vec!["A\r", "\r\n", "\r\n", "\r", "\r\n"],
            zero_before_finish: false,
        },
        Case {
            name: "malformed UTF8",
            input: vec![
                Input::Data(Stdout, &[240, 40, 140, 188]),
                Input::Data(Stderr, &[237, 160, 128]),
                Input::Data(Stdout, &[226, 130]),
                Input::Data(Stderr, b"Z"),
                Input::Data(Stdout, b"A"),
            ],
            display: vec!["�(��", "���", "Z", "�A"],
            zero_before_finish: false,
        },
        Case {
            name: "one ANSI parser across descriptors",
            input: vec![
                Input::Data(Stdout, b"\x1b["),
                Input::Data(Stderr, b"31mR"),
                Input::Data(Stdout, b"ESET\x1b[0m\n"),
            ],
            display: vec!["\x1b[", "31mR", "ESET\x1b[0m\r\n"],
            zero_before_finish: false,
        },
    ]
}
#[test]
fn actual_external_delivery_matches_node_stringdecoder_display_oracles() {
    // Expected callback bytes were generated with the ACTUAL existing Node
    // StringDecoder + AutomationRuns.display, including its combined close flush.
    for case in cases() {
        let identity = Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        };
        let mut runtime = Runtime::start_external_with_policy(
            identity,
            ExternalConfig::default(),
            ExternalDisplayPolicy::LegacyAutomationV1,
        )
        .unwrap();
        let (source, publisher) = AutomationView::new();
        let mut relay = AutomationRelay::new(source, &mut runtime).unwrap();
        let Response::Observed(observer) = super::relay::request(
            &runtime,
            1,
            Command::Observe {
                viewer: 1,
                after: Some(0),
            },
        ) else {
            panic!("Observer expected");
        };
        let mut sequence = 0;
        let mut eof = [false; 2];
        for input in case.input {
            let event = match input {
                Input::Data(stream, bytes) => {
                    sequence += 1;
                    AutomationViewEvent::Data {
                        sequence,
                        stream,
                        bytes: bytes.to_vec(),
                    }
                }
                Input::Eof(stream) => {
                    eof[usize::from(stream == Stderr)] = true;
                    AutomationViewEvent::Eof {
                        after_sequence: sequence,
                        stream,
                    }
                }
            };
            publisher.try_push(event).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut chunks = vec![];
        while runtime.external_status().unwrap().source_sequence != SourceSequence(sequence) {
            relay.pump().unwrap();
            collect(&runtime, &mut chunks);
            assert!(Instant::now() < deadline, "{}", case.name);
            std::thread::sleep(Duration::from_millis(1));
        }
        collect(&runtime, &mut chunks);
        if case.zero_before_finish {
            let status = runtime.external_status().unwrap();
            assert!(status.delivery.is_none());
            assert_eq!(status.last_delivery_sequence.0, 0);
            assert!(chunks.is_empty());
        }
        for stream in [Stdout, Stderr] {
            if !eof[usize::from(stream == Stderr)] {
                publisher
                    .try_push(AutomationViewEvent::Eof {
                        after_sequence: sequence,
                        stream,
                    })
                    .unwrap();
            }
        }
        publisher
            .try_push(AutomationViewEvent::Exit {
                after_sequence: sequence,
                code: 7,
            })
            .unwrap();
        while !relay.finished(&runtime) {
            relay.pump().unwrap();
            collect(&runtime, &mut chunks);
            assert!(Instant::now() < deadline, "{}", case.name);
            std::thread::sleep(Duration::from_millis(1));
        }
        while chunks.len() < case.display.len() {
            collect(&runtime, &mut chunks);
            assert!(Instant::now() < deadline, "{}", case.name);
            std::thread::sleep(Duration::from_millis(1));
        }
        let actual: Vec<_> = chunks
            .iter()
            .map(|bytes| std::str::from_utf8(bytes).unwrap())
            .collect();
        assert_eq!(actual, case.display, "{}", case.name);
        let status = runtime.external_status().unwrap();
        assert_eq!(status.source_sequence, SourceSequence(sequence));
        if case.zero_before_finish {
            let flush = status
                .finish_delivery
                .expect("Combined decoder close produced one explicit delivery");
            assert_eq!(flush.first.0, 1);
            assert_eq!(flush.last.0, 1);
        }
        assert!(!status.closed, "Finish must retain the canonical parser");
        let Response::Observed(_) = super::relay::request(
            &runtime,
            2,
            Command::Observe {
                viewer: 2,
                after: None,
            },
        ) else {
            panic!("Late observer expected");
        };
        runtime.request_external_close().unwrap();
        while !runtime.poll_external_closed().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let _ = observer;
    }
}
fn collect(runtime: &Runtime, chunks: &mut Vec<Vec<u8>>) {
    while let Ok(event) = runtime.try_recv() {
        match event {
            Event::ObserverOutput { chunk, .. } => chunks.push(chunk.bytes),
            Event::Started(Err(error)) => panic!("External parser start failed: {error}"),
            Event::Phase {
                error: Some(error), ..
            } => panic!("External parser failed: {error}"),
            _ => {}
        }
    }
}
