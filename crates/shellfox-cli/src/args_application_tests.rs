use super::*;
fn command(words: &[&str]) -> Command {
    let root = std::env::current_dir()
        .unwrap()
        .join("tmp")
        .join("parser-application-only");
    let mut values = vec![
        OsString::from("--data-dir"),
        root.clone().into_os_string(),
        "--helper".into(),
        root.join("owned-helper").into_os_string(),
        "--helper-sha256".into(),
        "0".repeat(64).into(),
        "--remote-hostname".into(),
        "owned-parser".into(),
    ];
    values.extend(words.iter().map(OsString::from));
    parse(values).unwrap().unwrap().command
}
#[test]
fn a_portable_pass_through_alias_is_not_a_configured_native_launcher() {
    let cwd = std::env::current_dir().unwrap();
    let bare = vec![OsString::from("start"), cwd.clone().into_os_string()];
    assert!(
        parse(bare).is_err(),
        "A path-only wrapper has no required data/helper launch context"
    );
    // Parser contract only. This helper supplies the mandatory explicit flags;
    // fake parser fixture hashes/paths are never installation-resource evidence.
    assert!(matches!(
        command(&["start", cwd.to_str().unwrap()]),
        Command::DesktopLaunch(_)
    ));
}
#[test]
fn power_commands_are_existing_local_host_requests_not_serve() {
    assert!(matches!(
        command(&["host", "power", "status"]),
        Command::Power(shellfox_service::power::PowerRequest::Status {})
    ));
    assert!(matches!(
        command(&["host", "power", "on"]),
        Command::Power(shellfox_service::power::PowerRequest::Set { active: true })
    ));
    assert!(matches!(
        command(&["host", "power", "off"]),
        Command::Power(shellfox_service::power::PowerRequest::Set { active: false })
    ));
    assert!(matches!(command(&["host", "serve"]), Command::Serve));
}
#[cfg(feature = "experimental-shellfox-native")]
#[test]
fn peer_creation_paths_are_not_reinterpreted_as_viewer_paths() {
    for expected in ["C:\\owned\\work", "/home/owned/work"] {
        let Command::Peer(args) = command(&["peer", "n-work", "create", "--cwd", expected]) else {
            panic!("peer route required")
        };
        let crate::peer_args::PeerCommand::Create { cwd, .. } = args.command else {
            panic!("create required")
        };
        assert_eq!(cwd, expected);
    }
}
