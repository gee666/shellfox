//! Actual Native0 capture/import with pending UTF-8, not a Ground-state substitute.
use super::*;
use shellfox_ghostty_vt::extension::{RestorationConfig, TerminalProfile, VerifiedNativeExtension};
use shellfox_ghostty_vt::{Selection, Terminal};
fn equal(source: &mut Terminal, manager: &ManagerRecovery<Transport>) {
    crate::engine::with_engine(manager.terminal, |owned| {
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
fn unicode_wide_combining_and_partial_utf8_recovery_preserve_real_suffix_and_history() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async{
        for pull in[false,true]{for(pending,completion)in[(b"\xe4\xb8".as_slice(),b"\xad".as_slice()),(b"\xf0\x9f\x98".as_slice(),b"\x80".as_slice())]{
            let(mut manager,transport)=manager().await;let verified=VerifiedNativeExtension::verify().unwrap();let configuration=RestorationConfig{runtime_image_storage_limit:10_000_000,runtime_resize_pull_scrollback:pull};let mut source=verified.new_native_terminal(20,5,TerminalProfile::Native,configuration,65536).unwrap();
            for _ in 0..9{source.feed("wide中 e\u{301}\r\n".as_bytes()).unwrap();}source.feed(b"\x1b[2;2H\x1b7\x1b[?1049h").unwrap();source.feed("alt界 a\u{308} ".as_bytes()).unwrap();source.feed(pending).unwrap();let before=source.continuation().unwrap();assert!(before.ends_with(pending),"actual pending UTF8 must not be Ground");
            let(snapshot,_)=source.encode_native_snapshot(&verified,SnapshotLimits::default(),configuration).expect("bounded Native0 must capture actual pending UTF8 or explicitly fail this acceptance case");assert_eq!(source.continuation().unwrap(),before);
            let value=attachment_snapshot(&transport,&snapshot);let m::Attachment::Attached(attached)=value.clone()else{panic!()};transport.ready(value);manager.attach(request()).await.unwrap();let prepared=manager.prepare_pending().unwrap();manager.commit_prepared(prepared).unwrap();equal(&mut source,&manager);manager.acknowledge_consumed().await.unwrap();
            for(sequence,bytes)in[completion,b"Z\x1b[?1049l\x1b8!".as_slice()].into_iter().enumerate(){source.feed(bytes).unwrap();transport.event(m::Event::Data{delivery:attached.delivery.clone(),sequence:sequence as u64+1,encoding:m::Encoding::Base64,data:shellfox_protocol::encode_chunk(bytes).unwrap()});manager.poll().await.unwrap();manager.pump().unwrap();equal(&mut source,&manager);manager.acknowledge_consumed().await.unwrap();}
            eprintln!("UNICODE_NATIVE0_CASE pull={pull} pending={pending:02x?} restored actual cells/cursor/history/continuation");manager.close().unwrap();
        }}
    });
}
