use shellfox_terminal::wsl::{
    decode_distro_list, GuestCommand, GuestSettings, WslProfileId, WslSecret,
};
#[test]
fn distro_ids_are_opaque_utf16_names_not_shell_arguments() {
    let bytes: Vec<u8> = "\u{feff}Ubuntu';&\r\n测试\r\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let names = decode_distro_list(&bytes).unwrap();
    assert_eq!(names[0].id(), "wsl:Ubuntu';&");
    assert_eq!(names[1].distro(), "测试");
    for bytes in [
        vec![0],
        vec![0, 0xd8],
        vec![0x41, 0, 0, 0],
        "D\nD\n".encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ] {
        assert!(decode_distro_list(&bytes).is_err());
    }
    assert!(WslProfileId::parse("wsl:").is_err());
    assert!(WslProfileId::parse("wsl:D\nother").is_err());
    assert!(GuestSettings::new("python3".into(), "/opt/trusted".into()).is_err());
    assert!(WslSecret::new([0; 32]).is_err());
    assert_eq!(
        format!("{:?}", WslSecret::new([1; 32]).unwrap()),
        "WslSecret([redacted])"
    );
    assert_eq!(GuestCommand::bash().args, ["--noprofile", "--norc", "-i"]);
}
#[test]
fn guest_sources_never_promote_nested_markers_or_scan_global_pids() {
    let tree = include_str!("../../../native/wsl/tree.py");
    assert!(!tree.contains("SHELLFOX_TERMINAL_MARKER"));
    assert!(!tree.contains("os.listdir('/proc')"));
    assert!(tree.contains("os.pidfd_open"));
    assert!(tree.contains("signal.pidfd_send_signal"));
    let host = include_str!("../src/wsl/discovery.rs");
    assert!(!host.contains("--terminate"));
    assert!(!host.contains("--import"));
    let legacy = include_str!("../../../src/main/terminal/nested-wsl-marker.test.ts");
    assert!(legacy.contains("never binds a direct WSL root from optional marker evidence"));
}

#[cfg(feature = "test-support")]
mod executable {
    use super::*;
    use shellfox_terminal::{
        wsl::{OsWsl, WslConfig, WslConsent, WslError, WslGuestGuardian, WslSession},
        PtyOutput,
    };
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };
    fn config(distro: &str) -> WslConfig {
        WslConfig {
            executable: OsWsl::fixture(PathBuf::from(env!(
                "CARGO_BIN_EXE_shellfox-wsl-test-helper"
            )))
            .unwrap(),
            distro: WslProfileId::parse(&format!("wsl:{distro}")).unwrap(),
            settings: GuestSettings::new("/usr/bin/python3".into(), "/opt/shellfox-fixture".into())
                .unwrap(),
            state_directory: "/tmp/shellfox-guest-fixture".into(),
            cwd: "/tmp".into(),
            command: GuestCommand::bash(),
            cols: 80,
            rows: 24,
            marker: "a".repeat(64),
            secret: WslSecret::new([1; 32]).unwrap(),
            consent: WslConsent::explicit_user_approval(),
        }
    }
    #[test]
    fn fake_wsl_owns_only_its_process_and_tunnels_raw_bytes_without_a_parser() {
        let config = config("Fixture");
        let credentials = config.clone();
        assert_eq!(config.executable.list().unwrap().len(), 2);
        let proof = WslGuestGuardian::fixture_preflight(
            &config.executable,
            &config.distro,
            &config.settings,
        )
        .unwrap();
        let mut session = WslSession::fixture_spawn(config, proof).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(Some(PtyOutput::Data(bytes))) = session.try_read() {
                assert_eq!(bytes, b"\x1b[31mguest\x1b[0m\r\n");
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let evidence = session.recovery().unwrap();
        assert_eq!(evidence.identity().unwrap().root_pid, 11);
        assert!(!format!("{evidence:?}").contains("010101"));
        let restored = shellfox_terminal::wsl::RecoveryEvidence::restore(
            credentials.settings,
            credentials.distro,
            credentials.secret,
            evidence.endpoint().into(),
            evidence.anchor().clone(),
            evidence.identity().cloned(),
        )
        .unwrap();
        assert_eq!(
            WslGuestGuardian::status(&credentials.executable, &restored)
                .unwrap()
                .identity
                .root_pid,
            11
        );
        session.write(b"exact input\r").unwrap();
        session.resize(100, 30).unwrap();
        session.shutdown().unwrap();
        assert!(session.write(b"late").is_err());
    }
    #[test]
    fn failed_close_retains_guest_authority_and_allows_exact_retry() {
        let config = config("CleanupFailure");
        let proof = WslGuestGuardian::fixture_preflight(
            &config.executable,
            &config.distro,
            &config.settings,
        )
        .unwrap();
        let mut session = WslSession::fixture_spawn(config, proof).unwrap();
        assert_eq!(session.shutdown(), Err(WslError::CleanupUnconfirmed));
        assert!(session.recovery().is_some());
        assert!(session.write(b"no replay").is_err());
        session.shutdown().unwrap();
    }
}
