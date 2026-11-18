#![cfg(feature = "experimental-shellfox-native")]
use shellfox_ghostty_vt::extension::*;
use shellfox_ghostty_vt::{Selection, Terminal};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn config(pull: bool) -> RestorationConfig {
    RestorationConfig {
        runtime_image_storage_limit: 10_000_000,
        runtime_resize_pull_scrollback: pull,
    }
}
fn native(v: &VerifiedNativeExtension, pull: bool) -> ExtensionResult<Terminal> {
    v.new_native_terminal(20, 5, TerminalProfile::Native, config(pull), 65536)
}
fn same(a: &mut Terminal, b: &mut Terminal) -> TestResult {
    let x = a.render()?;
    let y = b.render()?;
    assert_eq!(x.cells, y.cells);
    assert_eq!(x.cursor, y.cursor);
    assert_eq!(a.continuation()?, b.continuation()?);
    a.set_selection(Some(Selection::All))?;
    b.set_selection(Some(Selection::All))?;
    assert_eq!(a.copy_selection()?, b.copy_selection()?);
    a.set_selection(None)?;
    b.set_selection(None)?;
    Ok(())
}
#[test]
fn verification_and_explicit_legacy_refusal() -> TestResult {
    let verified = VerifiedNativeExtension::verify()?;
    let wrong = EngineIdentity::from_hex(&"f".repeat(64))?;
    assert!(matches!(
        VerifiedNativeExtension::verify_expected(wrong),
        Err(ExtensionError::IdentityMismatch)
    ));
    assert!(EngineIdentity::from_hex("short").is_err());
    assert!(matches!(
        verified.new_native_terminal(20, 5, TerminalProfile::LegacyXterm6, config(true), 65536),
        Err(ExtensionError::UnsupportedLegacy)
    ));
    Ok(())
}
#[test]
fn atomic_snapshot_adopts_existing_terminal_preserving_parser_and_live_suffix() -> TestResult {
    let verified = VerifiedNativeExtension::verify()?;
    let limits = SnapshotLimits::default();
    for pull in [false, true] {
        let mut source = native(&verified, pull)?;
        source.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\x1b[2;2H\x1b7")?;
        let normal_saved_cursor = source.render()?.cursor;
        source.feed(b"\x1b[?1049h\x1b[Halt\x1b[38;2;")?;
        let before = source.render()?;
        let continuation = source.continuation()?;
        let (snapshot, report) = source.encode_native_snapshot(&verified, limits, config(pull))?;
        assert_eq!(report.source_offset, 0);
        assert!(report.heap_bytes + report.mapped_bytes <= limits.max_allocation_bytes);
        let after_capture = source.render()?;
        assert_eq!(after_capture.cells, before.cells);
        assert_eq!(after_capture.cursor, before.cursor);
        assert_eq!(source.continuation()?, continuation);
        let incoming = NativeSnapshot::from_untrusted(
            &verified,
            snapshot.as_bytes(),
            snapshot.metadata().clone(),
            limits,
        )?;
        let (mut restored, report) = incoming.decode(&verified, limits)?;
        assert_eq!(report.source_offset, snapshot.as_bytes().len() as u64);
        assert_eq!(restored.continuation()?, continuation);
        same(&mut source, &mut restored)?;

        // Failed incoming recovery must not replace this already-live target.
        let mut unsupported = snapshot.metadata().clone();
        unsupported
            .state
            .as_mut()
            .ok_or("missing State")?
            .terminal_profile = TerminalProfile::LegacyXterm6;
        assert!(matches!(
            NativeSnapshot::from_untrusted(&verified, snapshot.as_bytes(), unsupported, limits),
            Err(ExtensionError::UnsupportedLegacy)
        ));
        same(&mut source, &mut restored)?;
        let mut mismatch = snapshot.metadata().clone();
        mismatch.engine_identity = EngineIdentity::from_hex(&"f".repeat(64))?;
        assert!(matches!(
            NativeSnapshot::from_untrusted(&verified, snapshot.as_bytes(), mismatch, limits),
            Err(ExtensionError::IdentityMismatch)
        ));
        same(&mut source, &mut restored)?;
        let truncated = NativeSnapshot::from_untrusted(
            &verified,
            &snapshot.as_bytes()[..snapshot.as_bytes().len() - 1],
            snapshot.metadata().clone(),
            limits,
        )?;
        assert!(truncated.decode(&verified, limits).is_err());
        same(&mut source, &mut restored)?;

        for bytes in [
            b"10;20;30m!".as_slice(),
            b"\x1b8Z".as_slice(),
            b"\x1b[?1049l!".as_slice(),
            b"\x1b[4;7H".as_slice(),
            b"\x1b8".as_slice(),
            b"Y".as_slice(),
        ] {
            source.feed(bytes)?;
            restored.feed(bytes)?;
            same(&mut source, &mut restored)?;
            if bytes == b"\x1b8" {
                // Normal-screen DECSC survives reconnect while the TUI was active.
                assert_eq!(source.render()?.cursor, normal_saved_cursor);
                assert_eq!(restored.render()?.cursor, normal_saved_cursor);
            }
            assert!(restored.drain_replies()?.is_empty());
            assert!(restored.take_metadata()?.is_none());
        }
        source.resize(30, 8)?;
        restored.resize(30, 8)?;
        same(&mut source, &mut restored)?;
        assert_eq!(restored.native_profile_state(&verified)?.preceding_join, 0);
    }
    Ok(())
}
#[test]
fn source_queries_and_metadata_are_not_drained_or_replayed_by_capture_adoption() -> TestResult {
    let verified = VerifiedNativeExtension::verify()?;
    let limits = SnapshotLimits::default();
    let mut source = Terminal::new(20, 5)?;
    source.feed(b"abc\x1b[6n\x1b]2;fixture\x07\x07")?;
    let (snapshot, _) = source.encode_native_snapshot(&verified, limits, config(true))?;
    assert_eq!(source.drain_replies()?, b"\x1b[1;4R");
    assert!(source.take_metadata()?.is_some());
    let (mut passive, _) = snapshot.decode(&verified, limits)?;
    passive.feed(b"\x1b[6n\x07\x1b]2;passive\x07\x1b]52;c;YQ==\x07")?;
    assert!(passive.drain_replies()?.is_empty());
    assert!(passive.take_metadata()?.is_none());
    assert!(source.drain_replies()?.is_empty());
    Ok(())
}
#[test]
fn adversarial_bodies_and_envelope_mismatches_never_publish_a_terminal() -> TestResult {
    let verified = VerifiedNativeExtension::verify()?;
    let limits = SnapshotLimits::default();
    let mut source = native(&verified, true)?;
    source.feed(b"owned fixture")?;
    let (valid, _) = source.encode_native_snapshot(&verified, limits, config(true))?;
    let metadata = valid.metadata().clone();
    // Actual LOCAL artifact through the explicit entry path. This is not an
    // actual foreign-target encode/decode proof, nor is sender metadata relabelled.
    let compatibility =
        Native0RawV1Compatibility::for_negotiated_sender(&verified, metadata.engine_identity);
    let imported = CrossTargetNative0RawV1Snapshot::from_untrusted(
        compatibility,
        valid.as_bytes(),
        metadata.clone(),
        limits,
    )?;
    assert_eq!(imported.metadata(), valid.metadata());
    assert_eq!(imported.as_bytes(), valid.as_bytes());
    let (mut previous, report) = imported.decode(&verified, limits)?;
    assert_eq!(report.source_offset, valid.as_bytes().len() as u64);
    same(&mut source, &mut previous)?;
    let wrong_sender = Native0RawV1Compatibility::for_negotiated_sender(
        &verified,
        EngineIdentity::from_hex(&"f".repeat(64))?,
    );
    assert!(matches!(
        CrossTargetNative0RawV1Snapshot::from_untrusted(
            wrong_sender,
            valid.as_bytes(),
            metadata.clone(),
            limits
        ),
        Err(ExtensionError::IdentityMismatch)
    ));
    same(&mut source, &mut previous)?;
    let zero = SnapshotLimits {
        max_allocation_bytes: 0,
        ..limits
    };
    assert!(imported.decode(&verified, zero).is_err());
    same(&mut source, &mut previous)?;
    let absurd = SnapshotLimits {
        max_encoded_bytes: usize::MAX,
        ..limits
    };
    assert!(CrossTargetNative0RawV1Snapshot::from_untrusted(
        compatibility,
        valid.as_bytes(),
        metadata.clone(),
        absurd
    )
    .is_err());
    same(&mut source, &mut previous)?;
    let mut trailing = valid.as_bytes().to_vec();
    trailing.push(0);
    let mut corrupt = valid.as_bytes().to_vec();
    corrupt[0] ^= 0xff;
    for body in [
        &[][..],
        &valid.as_bytes()[..valid.as_bytes().len() - 1],
        &trailing,
        &corrupt,
    ] {
        let input = NativeSnapshot::from_untrusted(&verified, body, metadata.clone(), limits)?;
        assert!(input.decode(&verified, limits).is_err());
        let incoming = CrossTargetNative0RawV1Snapshot::from_untrusted(
            compatibility,
            body,
            metadata.clone(),
            limits,
        )?;
        assert!(incoming.decode(&verified, limits).is_err());
        same(&mut source, &mut previous)?;
    }
    let mut missing = metadata.clone();
    missing.state = None;
    assert!(NativeSnapshot::from_untrusted(&verified, valid.as_bytes(), missing, limits).is_err());
    let mut legacy = metadata.clone();
    legacy
        .state
        .as_mut()
        .ok_or("missing State")?
        .terminal_profile = TerminalProfile::LegacyXterm6;
    assert!(matches!(
        NativeSnapshot::from_untrusted(&verified, valid.as_bytes(), legacy, limits),
        Err(ExtensionError::UnsupportedLegacy)
    ));
    let mut wrong = metadata;
    wrong.snapshot_version = 2;
    assert!(NativeSnapshot::from_untrusted(&verified, valid.as_bytes(), wrong, limits).is_err());
    source.feed(b" still live")?;
    previous.feed(b" still live")?;
    same(&mut source, &mut previous)?;
    assert!(previous.drain_replies()?.is_empty());
    assert!(previous.take_metadata()?.is_none());
    assert!(source.render()?.cells.iter().any(|cell| cell.text == "s"));
    Ok(())
}
#[test]
fn zero_budgets_and_wrong_configuration_fail_atomically_and_allow_retry() -> TestResult {
    let verified = VerifiedNativeExtension::verify()?;
    let limits = SnapshotLimits::default();
    let mut source = native(&verified, false)?;
    source.feed(b"retry fixture\x1b[")?;
    let before = source.continuation()?;
    let zero = SnapshotLimits {
        max_allocation_bytes: 0,
        ..limits
    };
    assert!(source
        .encode_native_snapshot(&verified, zero, config(false))
        .is_err());
    assert!(source
        .encode_native_snapshot(&verified, limits, config(true))
        .is_err());
    assert_eq!(source.continuation()?, before);
    let (valid, _) = source.encode_native_snapshot(&verified, limits, config(false))?;
    assert!(valid.decode(&verified, zero).is_err());
    let (mut restored, _) = valid.decode(&verified, limits)?;
    let compatibility = Native0RawV1Compatibility::for_negotiated_sender(
        &verified,
        valid.metadata().engine_identity,
    );
    let incoming = CrossTargetNative0RawV1Snapshot::from_untrusted(
        compatibility,
        valid.as_bytes(),
        valid.metadata().clone(),
        limits,
    )?;
    let (mut cross_target, _) = incoming.decode(&verified, limits)?;
    assert!(
        source
            .encode_native_snapshot(&verified, limits, config(true))
            .is_err(),
        "wrong actual source configuration must not produce recovery"
    );
    same(&mut source, &mut restored)?;
    same(&mut source, &mut cross_target)?;
    assert_eq!(restored.continuation()?, before);
    source.feed(b"2;2H!")?;
    restored.feed(b"2;2H!")?;
    cross_target.feed(b"2;2H!")?;
    same(&mut source, &mut restored)?;
    same(&mut source, &mut cross_target)?;
    Ok(())
}
#[test]
fn ansi_restore_constructs_fresh_passive_target_and_propagates_unsupported() -> TestResult {
    let verified = VerifiedNativeExtension::verify()?;
    let mut source = native(&verified, true)?;
    source.feed(b"primary\x1b[2;2H\x1b7\x1b[?1049h\x1b[Halt\x1b[4;5Hz\x1b[")?;
    let before = source.continuation()?;
    let bytes = source.export_native_ansi_fresh(&verified, AnsiLimits::default(), config(true))?;
    assert!(bytes.as_bytes().ends_with(&before));
    assert_eq!(source.continuation()?, before);
    let mut target = bytes.restore(&verified)?;
    same(&mut source, &mut target)?;
    source.feed(b"2;3H!\x1b[?1049l\x1b8Y")?;
    target.feed(b"2;3H!\x1b[?1049l\x1b8Y")?;
    same(&mut source, &mut target)?;
    assert!(target.drain_replies()?.is_empty());
    assert!(target.take_metadata()?.is_none());
    assert!(source
        .export_native_ansi_fresh(
            &verified,
            AnsiLimits {
                max_output_bytes: 2,
                ..Default::default()
            },
            config(true)
        )
        .is_err());
    Ok(())
}
