use super::*;
use crate::settings::api::ColorKind;
use std::ffi::OsString;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::current_dir()
            .unwrap()
            .join("tmp")
            .join(format!("device-prefs-{}", shellfox_core::SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn options(&self, extra: impl IntoIterator<Item = OsString>) -> Options {
        let options = self.plan(extra);
        crate::frontend_composition::prepare(&options).unwrap();
        options
    }
    fn plan(&self, extra: impl IntoIterator<Item = OsString>) -> Options {
        Options::parse(
            [
                OsString::from("--data-dir"),
                self.0.join("host").into_os_string(),
                "--desktop-dir".into(),
                self.0.join("ui").into_os_string(),
            ]
            .into_iter()
            .chain(extra),
        )
        .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn entries(path: &Path) -> Vec<std::ffi::OsString> {
    let mut names = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn sibling_device_persistence_keeps_last_three_per_kind_and_host_reopens_unchanged() {
    let scratch = Scratch::new();
    let options = scratch.options([]);
    assert_eq!(options.device_config_root, scratch.0.join("host.device"));
    assert!(!options.device_config_root.starts_with(&options.data_dir));
    // Public Host storage guard exercises the real metadata artifact allowlist.
    // No Service/Go owner, PTY or second application host is constructed.
    let host = shellfox_host::Host::open(&options.data_dir).unwrap();
    let before = host.snapshot().unwrap();
    drop(host);
    let host_entries = entries(&options.data_dir);
    let store = prepare_device(&options).unwrap();
    let another_window = store.clone();
    assert!(Arc::ptr_eq(&store, &another_window));
    for color in ["#123456", "#234567", "#345678", "#456789"] {
        store.save(ColorKind::Accent, color).unwrap();
    }
    store.save(ColorKind::Background, "#121212").unwrap();
    drop(another_window);
    drop(store);
    let later = options.for_window().unwrap();
    assert_ne!(options.run_dir, later.run_dir);
    assert_eq!(options.desktop_dir, later.desktop_dir);
    assert_eq!(options.device_config_root, later.device_config_root);
    let reopened = prepare_device(&later).unwrap();
    assert!(later.run_dir.is_dir());
    let saved = reopened.read().unwrap();
    assert_eq!(saved.accent, ["#234567", "#345678", "#456789"]);
    assert_eq!(saved.background, ["#121212"]);
    assert_eq!(host_entries, entries(&options.data_dir));
    let host = shellfox_host::Host::open(&options.data_dir).unwrap();
    assert_eq!(host.snapshot().unwrap().revision, before.revision);
    drop(host);
    assert!(
        !options
            .data_dir
            .join("shellfox-device-colors-v1.txt")
            .exists()
    );
}

#[test]
fn explicit_root_is_stable_and_cannot_overlap_host_or_ui_roots() {
    let scratch = Scratch::new();
    let explicit = scratch.0.join("frontend").join("device");
    let options = scratch.options([
        "--device-config-root".into(),
        explicit.clone().into_os_string(),
    ]);
    assert_eq!(options.device_config_root, explicit);
    prepare_device(&options).unwrap();
    let host = scratch.0.join("host");
    let ui = scratch.0.join("ui");
    for invalid in [
        host.clone(),
        host.join("device"),
        scratch.0.clone(),
        ui.clone(),
        ui.join("nested"),
    ] {
        assert!(configured_root(&host, &ui, Some(invalid)).is_err());
    }
    assert!(configured_root(&host, &ui, Some("relative".into())).is_err());
    assert!(
        configured_root(
            &host,
            &ui,
            Some(scratch.0.join("alias").join("..").join("device"))
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn symlink_device_root_is_rejected_without_writing_target() {
    let scratch = Scratch::new();
    let options = scratch.options([]);
    let outside = scratch.0.join("elsewhere");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &options.device_config_root).unwrap();
    assert!(prepare_device(&options).is_err());
    assert!(entries(&outside).is_empty());
}

#[cfg(unix)]
#[test]
fn symlink_ancestor_cannot_create_a_device_directory_in_an_outside_root() {
    let scratch = Scratch::new();
    let outside = scratch.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let alias = scratch.0.join("alias");
    std::os::unix::fs::symlink(&outside, &alias).unwrap();
    let options = scratch.plan([
        "--device-config-root".into(),
        alias.join("new-device").into_os_string(),
    ]);
    assert!(crate::frontend_composition::prepare(&options).is_err());
    assert!(entries(&outside).is_empty());
}

#[test]
fn retained_device_proof_refuses_a_replaced_plain_directory() {
    let scratch = Scratch::new();
    let options = scratch.options([]);
    let store = prepare_device(&options).unwrap();
    let original = scratch.0.join("original-device");
    match std::fs::rename(&options.device_config_root, &original) {
        Ok(()) => {
            std::fs::create_dir(&options.device_config_root).unwrap();
            assert!(store.save(ColorKind::Accent, "#123456").is_err());
            assert!(entries(&options.device_config_root).is_empty());
        }
        Err(error) if cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33)) => {
            // A live native directory handle may itself deny replacement.
            assert!(!original.exists());
            store.save(ColorKind::Accent, "#123456").unwrap();
        }
        Err(error) => panic!("Owned directory replacement fixture failed: {error}"),
    }
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    use std::os::windows::process::CommandExt;
    for path in [link, target] {
        assert!(!path.to_string_lossy().chars().any(|ch| matches!(
            ch,
            '&' | '|' | '<' | '>' | '^' | '%' | '!' | '"' | '\r' | '\n'
        )));
    }
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags(0x08000000)
        .output()
        .expect("Windows junction fixture needs cmd.exe and an NTFS-capable owned tmp directory");
    assert!(
        output.status.success(),
        "Owned junction fixture unavailable: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(windows)]
#[test]
fn windows_junction_root_and_ancestor_are_rejected_even_outside_overlap_roots() {
    {
        let scratch = Scratch::new();
        let outside = scratch.0.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let alias = scratch.0.join("alias");
        junction(&alias, &outside);
        let options = scratch.plan([
            "--device-config-root".into(),
            alias.join("new-device").into_os_string(),
        ]);
        assert!(crate::frontend_composition::prepare(&options).is_err());
        assert!(!options.desktop_dir.exists());
        assert!(!options.data_dir.exists());
        assert!(entries(&outside).is_empty());
        std::fs::remove_dir(&alias).unwrap();
    }
    for inside_host in [false, true] {
        let scratch = Scratch::new();
        let options = scratch.options([]);
        let target = if inside_host {
            options.data_dir.clone()
        } else {
            scratch.0.join("outside")
        };
        std::fs::create_dir_all(&target).unwrap();
        let before = entries(&target);
        junction(&options.device_config_root, &target);
        assert!(prepare_device(&options).is_err());
        assert_eq!(entries(&target), before);
        std::fs::remove_dir(&options.device_config_root).unwrap();
        let alias = scratch.0.join("alias");
        junction(&alias, &target);
        let nested = scratch.plan([
            "--device-config-root".into(),
            alias.join("nested-device").into_os_string(),
        ]);
        assert!(crate::frontend_composition::prepare(&nested).is_err());
        assert_eq!(entries(&target), before);
        std::fs::remove_dir(&alias).unwrap();
    }
}

fn prepare_device(options: &Options) -> Result<Arc<DeviceColors>> {
    let mut frontend = crate::frontend_composition::prepare(options)?;
    super::prepare(options, &mut frontend)
}
