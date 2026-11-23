use super::super::*;
use super::support::Fixture;
use shellfox_protocol::ErrorCode;

#[test]
fn explorer_disposition_is_explicit_and_default_keeps_the_existing_window_contract() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    let mut launch = fixture.resources.desktop_launch().unwrap();
    assert_eq!(launch.disposition, DesktopLaunchDisposition::ExistingWindow);
    let existing = explorer::command(&launch, false).unwrap();
    assert!(existing.ends_with("--new-session --cwd \"%1\\.\""));
    assert!(!existing.contains("--new-window"));
    assert!(!existing.contains("--request-id"));
    launch.disposition = DesktopLaunchDisposition::NewWindow;
    let new = explorer::command(&launch, true).unwrap();
    assert!(new.ends_with("--new-session --cwd \"%V\\.\" --new-window"));
    assert_eq!(new.matches("--new-window").count(), 1);
}
#[test]
fn fixed_prefix_cannot_pin_dedup_ids_or_change_native_intent() {
    let fixture = Fixture::new(SystemPlatform::Windows);
    for flag in [
        "--new-session",
        "--cwd",
        "--new-window",
        "--request-id",
        "--request-id=fixed-id",
        "--host=remote",
        "--open-session",
    ] {
        let mut launch = fixture.resources.desktop_launch().unwrap();
        launch.prefix_arguments = vec![flag.into()];
        assert_eq!(
            explorer::command(&launch, false).unwrap_err().code,
            ErrorCode::Validation,
            "{flag}"
        );
    }
    let mut launch = fixture.resources.desktop_launch().unwrap();
    launch.prefix_arguments = vec![
        "--data-dir".into(),
        fixture.root.to_string_lossy().into_owned(),
    ];
    assert!(explorer::command(&launch, false).is_ok());
}
