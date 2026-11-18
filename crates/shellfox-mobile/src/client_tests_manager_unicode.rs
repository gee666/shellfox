//! C ABI path uses real pending UTF8; suitable for the owned Android test executable.
use super::*;
use shellfox_ghostty_vt::{Selection, Terminal};
fn equal(source: &mut Terminal, engine: u64) {
    crate::engine::with_engine(engine, |owned| {
        let expected = source.render().unwrap();
        let actual = owned.terminal.render().unwrap();
        assert_eq!(actual.cells, expected.cells);
        assert_eq!(actual.cursor, expected.cursor);
        assert_eq!(
            owned.terminal.continuation().unwrap(),
            source.continuation().unwrap()
        );
        source.set_selection(Some(Selection::All)).unwrap();
        owned.terminal.set_selection(Some(Selection::All)).unwrap();
        assert_eq!(
            owned.terminal.copy_selection().unwrap(),
            source.copy_selection().unwrap()
        );
        source.set_selection(None).unwrap();
        owned.terminal.set_selection(None).unwrap();
        Ok(())
    })
    .unwrap();
}
#[test]
fn manager_cabi_preserves_wide_combining_history_and_mid_utf8_scalar() {
    for pull in [false, true] {
        for (pending, completion) in [
            (b"\xe4\xb8".as_slice(), b"\xad".as_slice()),
            (b"\xf0\x9f\x98".as_slice(), b"\x80".as_slice()),
        ] {
            let mut h = ManagerHarness::new();
            h.pair();
            let verified = VerifiedNativeExtension::verify().unwrap();
            let config = RestorationConfig {
                runtime_image_storage_limit: 10_000_000,
                runtime_resize_pull_scrollback: pull,
            };
            let mut source = verified
                .new_native_terminal(80, 24, TerminalProfile::Native, config, 65536)
                .unwrap();
            for row in 0..30 {
                source
                    .feed(format!("{row} 中 e\u{301}\r\n").as_bytes())
                    .unwrap();
            }
            source
                .feed("\x1b[2;3H\x1b7\x1b[?1049halt 界 ".as_bytes())
                .unwrap();
            source.feed(pending).unwrap();
            assert!(source.continuation().unwrap().ends_with(pending));
            let value = attachment_source(&mut source, config);
            let m::Attachment::Attached(a) = &value else {
                panic!()
            };
            let delivery = a.delivery.clone();
            let engine = h.bind_attach_value(value);
            h.ack(engine);
            equal(&mut source, engine);
            for (sequence, bytes) in [completion, b"Z\x1b[?1049l\x1b8!".as_slice()]
                .into_iter()
                .enumerate()
            {
                source.feed(bytes).unwrap();
                let mut operation = 0;
                assert_eq!(
                    unsafe { sfm_manager_v2_poll(h.h.handle, &mut operation) },
                    0
                );
                let request = h.h.request();
                let response = m::PollResponse {
                    events: vec![m::HostEvent::Terminal {
                        event: m::Event::Data {
                            delivery: delivery.clone(),
                            sequence: sequence as u64 + 1,
                            encoding: m::Encoding::Base64,
                            data: shellfox_protocol::encode_chunk(bytes).unwrap(),
                        },
                    }],
                };
                h.complete(&request, h.bound(serde_json::to_value(response).unwrap()));
                assert_eq!(h.h.event()["status"], 0);
                h.ack(engine);
                equal(&mut source, engine);
            }
            eprintln!("CABI_UNICODE_NATIVE0_CASE pull={pull} pending={pending:02x?} actualsource/receivercells,cursor,history,continuationequal");
            assert_eq!(sfm_manager_v2_unbind_terminal(h.h.handle, engine), 0);
            crate::engine::close(engine).unwrap();
        }
    }
}
