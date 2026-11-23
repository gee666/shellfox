use super::*;
use std::cell::{Cell, RefCell};
mod coordinator_tests;

#[derive(Default)]
struct Directories {
    calls: Cell<usize>,
}
impl LaunchDirectoryPolicy for Directories {
    fn validate_directory(&self, cwd: &str) -> Result<String, LaunchError> {
        self.calls.set(self.calls.get() + 1);
        if cwd.contains("missing") {
            Err(LaunchError::NotFound)
        } else {
            Ok(cwd.to_owned())
        }
    }
}
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
fn request() -> DesktopLaunchIntent {
    DesktopLaunchIntent::new(
        LaunchAction::NewSession {
            cwd: "/project space".into(),
        },
        LaunchDisposition::ExistingWindow,
    )
}
#[test]
fn old_start_aliases_resolve_in_invoking_process_and_legacy_flags_require_absolute_cwd() {
    let dirs = Directories::default();
    for (base, target, expected) in [
        ("C:\\project space", "..\\next", "C:\\next"),
        ("/work/project", "../next/.", "/work/next"),
    ] {
        for alias in ["start", "add", "new", "create"] {
            let intent = parse_launch_args(&strings(&[alias, target]), base, &dirs).unwrap();
            assert_eq!(
                intent.action,
                LaunchAction::NewSession {
                    cwd: expected.into()
                }
            );
        }
    }
    assert_eq!(
        parse_launch_args(&strings(&["start"]), "/work/project", &dirs)
            .unwrap()
            .action,
        LaunchAction::NewSession {
            cwd: "/work/project".into()
        }
    );
    assert_eq!(
        parse_launch_args(
            &strings(&["--new-session", "--cwd", "C:\\some folder\\."]),
            "/unused",
            &dirs
        )
        .unwrap()
        .action,
        LaunchAction::NewSession {
            cwd: "C:\\some folder".into()
        }
    );
    assert!(parse_launch_args(&[], "/unused", &dirs).unwrap().action == LaunchAction::Show {});
    let a = parse_launch_args(&strings(&["start"]), "/work", &dirs).unwrap();
    let b = parse_launch_args(&strings(&["start"]), "/work", &dirs).unwrap();
    assert_ne!(a.request_id, b.request_id);
}

#[test]
fn malformed_or_ambiguous_cli_does_not_reach_directory_or_host_transports() {
    let dirs = Directories::default();
    for args in [
        strings(&["--cwd", "/work"]),
        strings(&["--new-session"]),
        strings(&["--new-session", "--cwd", "relative"]),
        strings(&["start", ".", "extra"]),
        strings(&["start", ".", "--new-session"]),
        strings(&["--new-session", "--cwd", "/work", "start"]),
        strings(&["--new-window", "--new-window"]),
        strings(&["--run", "bad"]),
        strings(&["--launch-request-id", "not-a-uuid"]),
        strings(&["start", "https://remote"]),
    ] {
        assert!(
            parse_launch_args(&args, "/work", &dirs).is_err(),
            "{args:?}"
        );
    }
    assert_eq!(dirs.calls.get(), 0);
    assert_eq!(
        parse_launch_args(&strings(&["start", "missing"]), "/work", &dirs),
        Err(LaunchError::NotFound)
    );
}

#[test]
fn windows_wsl_and_posix_path_forms_stay_separate_without_ambient_cwd_or_home() {
    assert_eq!(
        path::resolve_launch_path("C:\\work", ".\\child\\..\\next").unwrap(),
        "C:\\work\\next"
    );
    assert_eq!(
        path::normalize_launch_path("\\\\wsl.localhost\\Ubuntu\\work\\.\\child\\..").unwrap(),
        "\\\\wsl.localhost\\Ubuntu\\work"
    );
    assert_eq!(
        path::resolve_launch_path("/work", "~/project").unwrap(),
        "/work/~/project"
    );
    assert_eq!(
        path::resolve_launch_path("C:\\work", "C:relative").unwrap(),
        "C:\\work\\relative"
    );
    assert_eq!(
        path::resolve_launch_path("C:\\work", "\\root-relative").unwrap(),
        "C:\\root-relative"
    );
    assert_eq!(path::resolve_launch_path("/work", "").unwrap(), "/work");
    for target in [
        "D:relative",
        "\\\\server\\share",
        "C:\\work:stream",
        "C:\\work\nother",
    ] {
        assert!(path::resolve_launch_path("C:\\work", target).is_err());
    }
    assert_eq!(path::normalize_launch_path("/").unwrap(), "/");
    assert_eq!(path::normalize_launch_path("C:\\").unwrap(), "C:\\");
}
