use super::*;
use std::{ffi::OsString, path::PathBuf};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::current_dir().unwrap().join("tmp").join(format!(
            "frontend-planning-{}",
            shellfox_core::SessionId::new()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn args(&self) -> Vec<OsString> {
        vec![
            "--fixture".into(),
            "--data-dir".into(),
            self.0.join("host").into_os_string(),
            "--desktop-dir".into(),
            self.0.join("ui").into_os_string(),
        ]
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn planning_and_inspection_create_nothing_and_fixture_composition_is_frontend_only() {
    let scratch = Scratch::new();
    let options = Options::parse(scratch.args()).unwrap();
    let another = options.for_window().unwrap();
    inspect(&options).unwrap();
    for path in [
        &options.data_dir,
        &options.desktop_dir,
        &options.device_config_root,
        &options.run_dir,
        &another.run_dir,
    ] {
        assert!(!path.exists());
    }
    let _proofs = prepare(&options).unwrap();
    assert!(options.desktop_dir.is_dir());
    assert!(options.run_dir.is_dir());
    assert!(!options.data_dir.exists());
    assert!(!options.device_config_root.exists());
    assert!(!another.run_dir.exists());
}
fn editor_if_reviewed(frontend: &mut FrontendDirectories) -> bool {
    match crate::editor_composition::prepare(frontend) {
        crate::editor_composition::EditorOutcome::Ready(_) => true,
        crate::editor_composition::EditorOutcome::Unavailable(reason) => {
            panic!("Editor composition failed: {}", reason.message())
        }
    }
}
#[test]
fn preexisting_frontend_state_directories_are_repaired_before_use() {
    let scratch = Scratch::new();
    let options = Options::parse(scratch.args()).unwrap();
    let editor = options.desktop_dir.join("file-editor");
    std::fs::create_dir_all(&editor).unwrap();
    std::fs::create_dir(&options.device_config_root).unwrap();
    #[cfg(unix)]
    for path in [&options.desktop_dir, &editor, &options.device_config_root] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o775)).unwrap();
    }
    let mut frontend = prepare(&options).unwrap();
    assert!(editor_if_reviewed(&mut frontend));
    assert!(crate::device_prefs::prepare(&options, &mut frontend).is_ok());
    assert!(!options.data_dir.exists());
}

#[test]
fn options_to_real_frontend_composition_keeps_default_and_explicit_ui_outside_host() {
    for explicit_ui in [false, true] {
        let scratch = Scratch::new();
        let host = scratch.0.join("host");
        let mut args = vec![
            OsString::from("--fixture"),
            "--data-dir".into(),
            host.clone().into_os_string(),
        ];
        let expected_ui = if explicit_ui {
            scratch.0.join("frontend")
        } else {
            scratch.0.join("host.desktop")
        };
        if explicit_ui {
            args.extend(["--desktop-dir".into(), expected_ui.clone().into_os_string()]);
        }
        let options = Options::parse(args).unwrap();
        assert_eq!(options.desktop_dir, expected_ui);
        assert_eq!(options.run_dir.parent(), Some(expected_ui.as_path()));
        assert_eq!(options.device_config_root, scratch.0.join("host.device"));
        let mut frontend = prepare(&options).unwrap();
        let colors = crate::device_prefs::prepare(&options, &mut frontend).unwrap();
        let editor_enabled = editor_if_reviewed(&mut frontend);
        frontend.revalidate().unwrap();
        colors
            .save(crate::settings::api::ColorKind::Accent, "#123456")
            .unwrap();
        assert!(options.run_dir.is_dir());
        assert_eq!(expected_ui.join("file-editor").exists(), editor_enabled);
        assert!(
            options
                .device_config_root
                .join("shellfox-device-colors-v1.txt")
                .is_file()
        );
        assert!(
            !host.exists(),
            "Frontend composition must not require any Host-root artifact allowance"
        );
    }
}
#[test]
fn multiple_missing_prefixes_and_for_window_keep_siblings_and_independent_issuers() {
    let scratch = Scratch::new();
    let host = scratch.0.join("missing-a").join("missing-b").join("host");
    let options = Options::parse([
        OsString::from("--fixture"),
        "--data-dir".into(),
        host.clone().into_os_string(),
    ])
    .unwrap();
    let another = options.for_window().unwrap();
    assert!(!host.parent().unwrap().exists());
    assert_eq!(options.desktop_dir, host.with_file_name("host.desktop"));
    assert_eq!(
        options.device_config_root,
        host.with_file_name("host.device")
    );
    assert_ne!(options.run_dir, another.run_dir);
    let mut first = FrontendDirectories::plan(&options).unwrap();
    let original = first.plans.files.clone();
    first.prepare().unwrap();
    let colors = crate::device_prefs::prepare(&options, &mut first).unwrap();
    colors
        .save(crate::settings::api::ColorKind::Accent, "#123456")
        .unwrap();
    first.revalidate().unwrap();
    assert!(!another.run_dir.exists());
    let _preserved = first.preserve();
    // A completed preparation must not revoke this independent derived scope.
    assert_eq!(colors.read().unwrap().accent, ["#123456"]);
    let mut second = FrontendDirectories::plan(&another).unwrap();
    assert!(!Arc::ptr_eq(&original, &second.plans.files));
    second.prepare().unwrap();
    let reopened = crate::device_prefs::prepare(&another, &mut second).unwrap();
    assert_eq!(reopened.read().unwrap().accent, ["#123456"]);
    second.revalidate().unwrap();
    assert!(options.run_dir.is_dir());
    assert!(another.run_dir.is_dir());
    assert_eq!(options.desktop_dir, another.desktop_dir);
    assert_eq!(options.device_config_root, another.device_config_root);
    assert!(!options.desktop_dir.join("file-editor").exists());
    assert!(
        !host.exists(),
        "Frontend composition must never create the Host root"
    );
    let _preserved_second = second.preserve();
}
#[test]
fn options_rejects_ui_roots_inside_equal_to_or_ancestors_of_host_or_device() {
    let scratch = Scratch::new();
    let host = scratch.0.join("host");
    for ui in [
        host.clone(),
        host.join("desktop"),
        scratch.0.clone(),
        scratch.0.join("host.device"),
    ] {
        assert!(
            Options::parse([
                OsString::from("--data-dir"),
                host.clone().into_os_string(),
                "--desktop-dir".into(),
                ui.into_os_string()
            ])
            .is_err()
        );
        assert!(!host.exists());
    }
}
#[test]
fn revocation_before_creation_cannot_create_any_frontend_root() {
    let scratch = Scratch::new();
    let options = Options::parse(scratch.args()).unwrap();
    let mut frontend = FrontendDirectories::plan(&options).unwrap();
    frontend.plans.files.revoke().unwrap();
    assert!(frontend.prepare().is_err());
    let _preserved = frontend.preserve();
    assert!(!options.desktop_dir.exists());
    assert!(!options.device_config_root.exists());
    assert!(!options.data_dir.exists());
}
#[test]
fn unrecorded_creation_cannot_be_adopted_by_an_original_missing_plan() {
    let scratch = Scratch::new();
    let options = Options::parse(scratch.args()).unwrap();
    let mut frontend = prepare(&options).unwrap();
    let device = &options.device_config_root;
    std::fs::create_dir(device).unwrap();
    assert!(frontend.device_root().is_err());
    assert!(std::fs::read_dir(device).unwrap().next().is_none());
    assert!(!options.desktop_dir.join("file-editor").exists());
    assert!(!options.data_dir.exists());
}
#[test]
fn device_keeps_original_authority_with_reviewed_editor_composition() {
    let scratch = Scratch::new();
    let options = Options::parse(scratch.args()).unwrap();
    let mut frontend = prepare(&options).unwrap();
    let authority = frontend.plans.files.clone();
    let colors = crate::device_prefs::prepare(&options, &mut frontend).unwrap();
    let editor_enabled = editor_if_reviewed(&mut frontend);
    assert_eq!(
        options.desktop_dir.join("file-editor").exists(),
        editor_enabled
    );
    drop(frontend);
    authority.revoke().unwrap();
    assert!(
        colors
            .save(crate::settings::api::ColorKind::Accent, "#123456")
            .is_err()
    );
}
#[cfg(windows)]
#[test]
fn windows_case_alias_and_ambiguous_missing_names_fail_before_frontend_mkdir() {
    {
        let scratch = Scratch::new();
        let mut options = Options::parse(scratch.args()).unwrap();
        std::fs::create_dir(&options.data_dir).unwrap();
        options.desktop_dir = scratch.0.join("HOST");
        assert!(
            options.desktop_dir.is_dir(),
            "Owned test needs case-insensitive directory naming"
        );
        options.run_dir = options.desktop_dir.join("run");
        assert!(prepare(&options).is_err());
        assert!(
            std::fs::read_dir(&options.data_dir)
                .unwrap()
                .next()
                .is_none()
        );
        assert!(!options.device_config_root.exists());
    }
    let scratch = Scratch::new();
    let options = Options::parse([
        "--fixture".into(),
        "--data-dir".into(),
        scratch.0.join("host").into_os_string(),
        "--desktop-dir".into(),
        scratch.0.join("UI~1").into_os_string(),
    ])
    .unwrap();
    assert!(prepare(&options).is_err());
    assert!(!options.desktop_dir.exists());
    assert!(!options.data_dir.exists());
    assert!(!options.device_config_root.exists());
}
#[cfg(unix)]
#[test]
fn configured_outside_alias_rejects_before_any_frontend_creation() {
    let scratch = Scratch::new();
    let outside = scratch.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let alias = scratch.0.join("alias");
    std::os::unix::fs::symlink(&outside, &alias).unwrap();
    let mut args = scratch.args();
    args.extend([
        "--device-config-root".into(),
        alias.join("device").into_os_string(),
    ]);
    let options = Options::parse(args).unwrap();
    assert!(prepare(&options).is_err());
    assert!(!options.desktop_dir.exists());
    assert!(!options.data_dir.exists());
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
}
