//! Explicit cross-target acceptance fixture. Not network authentication or a new wire codec.
#![cfg(feature = "experimental-shellfox-native")]
use shellfox_ghostty_vt::extension::*;
use shellfox_ghostty_vt::{Selection, Terminal};
use std::{error::Error, fs, io::Write, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const INITIAL: &[u8] =
    b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\x1b[2;2H\x1b7\x1b[?1049h\x1b[Halt\x1b[38;2;";
fn config(pull: bool) -> RestorationConfig {
    RestorationConfig {
        runtime_image_storage_limit: 10_000_000,
        runtime_resize_pull_scrollback: pull,
    }
}
fn source(verified: &VerifiedNativeExtension, pull: bool) -> Result<Terminal> {
    let mut terminal =
        verified.new_native_terminal(20, 5, TerminalProfile::Native, config(pull), 65536)?;
    terminal.feed(INITIAL)?;
    Ok(terminal)
}
fn same(left: &mut Terminal, right: &mut Terminal) -> Result<()> {
    let a = left.render()?;
    let b = right.render()?;
    assert_eq!(a.cells, b.cells);
    assert_eq!(a.cursor, b.cursor);
    assert_eq!(left.continuation()?, right.continuation()?);
    left.set_selection(Some(Selection::All))?;
    right.set_selection(Some(Selection::All))?;
    assert_eq!(left.copy_selection()?, right.copy_selection()?);
    left.set_selection(None)?;
    right.set_selection(None)?;
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
// Test-only companion metadata. Raw snapshot bytes are unchanged existing raw-v1.
fn metadata_text(metadata: &SnapshotMetadata) -> Result<String> {
    let state = metadata.state.ok_or("missing captured State")?;
    let profile = match state.terminal_profile {
        TerminalProfile::Native => 0,
        TerminalProfile::LegacyXterm6 => 1,
    };
    Ok(format!(
        "SHELLFOX-NATIVE0-INTEROP-TEST-1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        std::str::from_utf8(metadata.engine_identity.as_bytes())?,
        metadata.wire_profile,
        metadata.library_version,
        metadata.snapshot_version,
        state.abi_version,
        profile,
        state.preceding_join,
        metadata.configuration.runtime_image_storage_limit,
        metadata.configuration.runtime_resize_pull_scrollback
    ))
}
fn read_metadata(path: &Path) -> Result<SnapshotMetadata> {
    if fs::metadata(path)?.len() > 4096 {
        return Err("oversized fixture metadata".into());
    }
    let text = fs::read_to_string(path)?;
    let fields: Vec<_> = text.lines().collect();
    if fields.len() != 10 || fields[0] != "SHELLFOX-NATIVE0-INTEROP-TEST-1" {
        return Err("unknown fixture metadata".into());
    }
    let profile = match fields[6] {
        "0" => TerminalProfile::Native,
        "1" => TerminalProfile::LegacyXterm6,
        _ => return Err("unknown profile".into()),
    };
    Ok(SnapshotMetadata {
        engine_identity: EngineIdentity::from_hex(fields[1])?,
        wire_profile: fields[2].parse()?,
        library_version: fields[3].parse()?,
        snapshot_version: fields[4].parse()?,
        state: Some(ProfileState {
            abi_version: fields[5].parse()?,
            terminal_profile: profile,
            preceding_join: fields[7].parse()?,
        }),
        configuration: RestorationConfig {
            runtime_image_storage_limit: fields[8].parse()?,
            runtime_resize_pull_scrollback: fields[9].parse()?,
        },
    })
}
#[test]
#[ignore = "explicit produce/consume mode and owned cross-target fixture directory required"]
fn actual_cross_target_native0_snapshot() -> Result<()> {
    let verified = VerifiedNativeExtension::verify()?;
    let mode = std::env::var("SHELLFOX_NATIVE_CROSS_MODE")?;
    let directory = std::path::PathBuf::from(std::env::var("SHELLFOX_NATIVE_CROSS_DIR")?);
    #[cfg(not(target_os = "android"))]
    let allowed = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .canonicalize()?;
    #[cfg(target_os = "android")]
    let allowed = {
        let value = std::env::var("SHELLFOX_NATIVE_CROSS_ALLOWED_ROOT")?;
        if !value.starts_with("/data/local/tmp/shellfox-native-cross-") {
            return Err("expected an agent-owned Android fixture root".into());
        }
        Path::new(&value).canonicalize()?
    };
    fs::create_dir_all(&directory)?;
    let directory = directory.canonicalize()?;
    if !directory.starts_with(&allowed) {
        return Err("fixture must remain in project tmp".into());
    }
    let limits = SnapshotLimits::default();
    let local_id = std::str::from_utf8(verified.identity().as_bytes())?.to_owned();
    if mode == "produce" {
        for pull in [false, true] {
            let mut terminal = source(&verified, pull)?;
            let (snapshot, _) = terminal.encode_native_snapshot(&verified, limits, config(pull))?;
            write_new(
                &directory.join(format!("cut-{pull}.raw")),
                snapshot.as_bytes(),
            )?;
            write_new(
                &directory.join(format!("cut-{pull}.meta")),
                metadata_text(snapshot.metadata())?.as_bytes(),
            )?;
        }
        write_new(&directory.join("producer-engine.txt"), local_id.as_bytes())?;
        println!("ACTUAL_NATIVE0_PRODUCER_ENGINE={local_id}");
        return Ok(());
    }
    if mode != "consume" {
        return Err("expected produce or consume".into());
    }
    // Supplied independently by the acceptance runner, never derived from incoming metadata.
    let sender =
        EngineIdentity::from_hex(&std::env::var("SHELLFOX_NATIVE_CROSS_EXPECTED_SENDER")?)?;
    assert_ne!(
        sender,
        verified.identity(),
        "cross-target test requires actual different build identities"
    );
    for pull in [false, true] {
        let metadata = read_metadata(&directory.join(format!("cut-{pull}.meta")))?;
        assert_eq!(metadata.engine_identity, sender);
        let path = directory.join(format!("cut-{pull}.raw"));
        if fs::metadata(&path)?.len() > u64::try_from(limits.max_encoded_bytes)? {
            return Err("oversized fixture body".into());
        }
        let bytes = fs::read(path)?;
        assert!(matches!(
            NativeSnapshot::from_untrusted(&verified, &bytes, metadata.clone(), limits),
            Err(ExtensionError::IdentityMismatch)
        ));
        let policy = Native0RawV1Compatibility::for_negotiated_sender(&verified, sender);
        let incoming = CrossTargetNative0RawV1Snapshot::from_untrusted(
            policy,
            &bytes,
            metadata.clone(),
            limits,
        )?;
        let (mut decoded, report) = incoming.decode(&verified, limits)?;
        assert_eq!(report.source_offset, u64::try_from(bytes.len())?);
        let mut reference = source(&verified, pull)?;
        same(&mut reference, &mut decoded)?;
        let wrong = Native0RawV1Compatibility::for_negotiated_sender(
            &verified,
            EngineIdentity::from_hex(&"f".repeat(64))?,
        );
        assert!(CrossTargetNative0RawV1Snapshot::from_untrusted(
            wrong,
            &bytes,
            metadata.clone(),
            limits
        )
        .is_err());
        let mut corrupt = bytes.clone();
        corrupt[0] ^= 0xff;
        let bad = CrossTargetNative0RawV1Snapshot::from_untrusted(
            policy,
            &corrupt,
            metadata.clone(),
            limits,
        )?;
        assert!(bad.decode(&verified, limits).is_err());
        let zero = SnapshotLimits {
            max_allocation_bytes: 0,
            ..limits
        };
        assert!(incoming.decode(&verified, zero).is_err());
        same(&mut reference, &mut decoded)?;
        for suffix in [
            b"10;20;30m!".as_slice(),
            b"\x1b8Z".as_slice(),
            b"\x1b[?1049l!".as_slice(),
            b"\x1b[4;7H".as_slice(),
            b"\x1b8".as_slice(),
            b"Y".as_slice(),
        ] {
            reference.feed(suffix)?;
            decoded.feed(suffix)?;
            same(&mut reference, &mut decoded)?;
            assert!(decoded.drain_replies()?.is_empty());
            assert!(decoded.take_metadata()?.is_none());
        }
        reference.resize(30, 8)?;
        decoded.resize(30, 8)?;
        same(&mut reference, &mut decoded)?;
    }
    println!(
        "ACTUAL_NATIVE0_CROSS_DECODE receiver={local_id}; sender={}",
        std::str::from_utf8(sender.as_bytes())?
    );
    Ok(())
}
