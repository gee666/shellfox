use super::*;
use crate::app_launch::{LaunchAction, LaunchDisposition};
use std::path::Path;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::current_dir().unwrap().join("tmp").join(format!(
            "desktop-launch-options-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join("project")).unwrap();
        Self(path)
    }
    fn args(&self) -> Vec<OsString> {
        vec![
            "--data-dir".into(),
            self.0.join("host").into_os_string(),
            "--desktop-dir".into(),
            self.0.join("ui").into_os_string(),
        ]
    }
    fn parse(&self, extra: impl IntoIterator<Item = OsString>) -> Result<Options> {
        Options::parse(self.args().into_iter().chain(extra))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bare_launch_is_show_and_new_session_preserves_request_and_disposition() {
    let scratch = Scratch::new();
    let show = scratch.parse([]).unwrap();
    assert!(matches!(show.launch.action, LaunchAction::Show {}));
    assert_eq!(show.launch.disposition, LaunchDisposition::ExistingWindow);
    let request = shellfox_core::SessionId::new();
    let project = scratch.0.join("project");
    let options = scratch
        .parse([
            "--new-session".into(),
            "--cwd".into(),
            project.clone().into_os_string(),
            "--new-window".into(),
            "--launch-request-id".into(),
            request.to_string().into(),
        ])
        .unwrap();
    assert_eq!(options.launch.request_id, request);
    assert_eq!(options.launch.disposition, LaunchDisposition::NewWindow);
    assert!(
        matches!(options.launch.action, LaunchAction::NewSession { cwd } if Path::new(&cwd) == project.as_path())
    );
}

#[test]
fn launch_pair_is_strict_and_cannot_leak_into_fixture_or_smoke() {
    let scratch = Scratch::new();
    for extra in [
        vec!["--new-session".into()],
        vec!["--cwd".into(), scratch.0.join("project").into_os_string()],
        vec!["--new-session".into(), "--cwd".into(), "relative".into()],
        vec!["--new-window".into(), "--new-window".into()],
        vec!["--fixture".into(), "--new-window".into()],
        vec!["--smoke".into(), "--new-window".into()],
    ] {
        assert!(scratch.parse(extra).is_err());
    }
}

#[test]
fn aliases_resolve_against_the_invoking_cwd_after_global_argument_separation() {
    let scratch = Scratch::new();
    let project = scratch.0.join("project");
    let invoking = std::env::current_dir().unwrap();
    let relative = project.strip_prefix(&invoking).unwrap();
    for alias in ["start", "add", "new", "create"] {
        let options = scratch
            .parse([
                alias.into(),
                relative.as_os_str().to_owned(),
                "--new-window".into(),
            ])
            .unwrap();
        assert!(
            matches!(options.launch.action, LaunchAction::NewSession { cwd } if Path::new(&cwd) == project.as_path())
        );
    }
}
