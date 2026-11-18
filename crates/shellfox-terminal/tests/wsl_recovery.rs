use shellfox_terminal::wsl::{
    GuardianAnchor, GuestIdentity, GuestSettings, RecoveryEvidence, WslProfileId, WslSecret,
};
fn anchor() -> GuardianAnchor {
    GuardianAnchor {
        boot_id: "12345678-1234-1234-1234-123456789abc".into(),
        guardian_pid: 10,
        guardian_ticks: "100".into(),
        marker: "a".repeat(64),
    }
}
fn identity() -> GuestIdentity {
    GuestIdentity {
        boot_id: anchor().boot_id,
        guardian_pid: 10,
        guardian_ticks: "100".into(),
        root_pid: 11,
        root_ticks: "101".into(),
        session: 11,
        marker: "a".repeat(64),
    }
}
fn restore(
    endpoint: &str,
    anchor: GuardianAnchor,
    identity: Option<GuestIdentity>,
) -> Result<RecoveryEvidence, shellfox_terminal::wsl::WslError> {
    RecoveryEvidence::restore(
        GuestSettings::new("/usr/bin/python3".into(), "/opt/trusted/wsl".into()).unwrap(),
        WslProfileId::parse("wsl:Fixture").unwrap(),
        WslSecret::new([1; 32]).unwrap(),
        endpoint.into(),
        anchor,
        identity,
    )
}
#[test]
fn private_records_do_not_become_liveness_or_marker_only_ownership() {
    assert!(restore(
        "/tmp/shellfox-private/control.sock",
        anchor(),
        Some(identity())
    )
    .is_ok());
    assert!(restore("/tmp/shellfox-private/control.sock", anchor(), None).is_ok());
    for endpoint in [
        "relative/control.sock",
        "/tmp/../other/control.sock",
        "/tmp/not-control.sock",
        "/tmp/control.sock\n",
    ] {
        assert!(restore(endpoint, anchor(), Some(identity())).is_err());
    }
    let mut changed = identity();
    changed.root_ticks = "not-a-birth".into();
    assert!(restore("/tmp/owned/control.sock", anchor(), Some(changed)).is_err());
    let mut changed = anchor();
    changed.guardian_ticks = "999".into();
    assert!(restore("/tmp/owned/control.sock", changed, Some(identity())).is_err());
    let mut changed = anchor();
    changed.marker = "01".repeat(32);
    assert!(restore("/tmp/owned/control.sock", changed, None).is_err());
}
