use shellfox_core::ProfileId;
use shellfox_service::{AUTOMATION_PROFILE_ID, AUTOMATION_READONLY_PROFILE};

#[test]
fn automation_marker_matches_committed_native_ts_and_cli_metadata_contract() {
    assert_eq!(AUTOMATION_PROFILE_ID, "automation-pipe");
    assert_eq!(AUTOMATION_READONLY_PROFILE, AUTOMATION_PROFILE_ID);
    let profile = ProfileId::new(AUTOMATION_PROFILE_ID).unwrap();
    assert_eq!(profile.as_str(), "automation-pipe");
    let json = serde_json::to_string(&profile).unwrap();
    assert_eq!(json, "\"automation-pipe\"");
    let restored: ProfileId = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, profile);
    // Marker/profile parsing supplies no Runtime, observer, lease or authority.
    assert_ne!(ProfileId::new("cmd").unwrap(), restored);
}
