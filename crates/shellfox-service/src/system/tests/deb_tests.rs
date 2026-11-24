use super::super::*;
use super::support::Environment;
use shellfox_protocol::ErrorCode;
use std::{fs, path::Path, sync::Arc};

const LAUNCHER: &str = "#!/bin/sh\n# Shellfox/linux-v1\n# synthetic; never executed\n";

/// A fake package and HOME under tmp/, with Nemo and Thunar on PATH.
struct Deb {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
}
impl Deb {
    fn new() -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        fs::create_dir_all(project.join("tmp")).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("deb-menus-")
            .tempdir_in(project.join("tmp"))
            .unwrap();
        let root = temp.path().to_owned();
        for dir in ["usr/lib/shellfox", "usr/bin", "programs", "home"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("usr/lib/shellfox/shellfox"), b"synthetic desktop").unwrap();
        fs::write(root.join("usr/bin/shellfox"), LAUNCHER).unwrap();
        for program in ["nemo", "thunar"] {
            fs::write(root.join("programs").join(program), b"").unwrap();
        }
        // The package installs these 0755; the CLI check requires it on Unix.
        #[cfg(unix)]
        for path in [
            root.join("usr/bin/shellfox"),
            root.join("programs/nemo"),
            root.join("programs/thunar"),
        ] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { _temp: temp, root }
    }
    fn services(&self) -> HostSystemServices {
        let home = self.root.join("home");
        let environment = Arc::new(Environment {
            platform: SystemPlatform::Linux,
            home: home.clone(),
            variables: [
                ("PATH".into(), self.root.join("programs").into_os_string()),
                (
                    "XDG_DATA_DIRS".into(),
                    self.root.join("share").into_os_string(),
                ),
                ("XDG_DATA_HOME".into(), home.join("data").into_os_string()),
                (
                    "XDG_CONFIG_HOME".into(),
                    home.join("config").into_os_string(),
                ),
            ]
            .into_iter()
            .collect(),
        });
        let package = DebPackage::at(
            self.root.join("usr/lib/shellfox"),
            self.root.join("usr/bin"),
        );
        HostSystemServices::new(Arc::new(package.backend(environment)))
    }
    fn nemo(&self) -> std::path::PathBuf {
        self.root
            .join("home/data/nemo/actions/shellfox.nemo_action")
    }
}

#[test]
fn deb_menus_are_installed_once_and_call_the_packaged_launcher() {
    let deb = Deb::new();
    let services = deb.services();
    assert!(services.ensure_cli().unwrap().installed);
    assert!(
        !services
            .explorer_integration(SystemScope::User)
            .unwrap()
            .installed
    );
    assert!(services.ensure_explorer().unwrap().installed);
    let nemo = fs::read_to_string(deb.nemo()).unwrap();
    let launcher = deb.root.join("usr/bin/shellfox");
    assert!(nemo.contains("# Shellfox/linux-v1"));
    assert!(nemo
        .lines()
        .any(|line| line.starts_with("Exec=\"") && line.ends_with("shellfox\" start %F")));
    let thunar = fs::read_to_string(deb.root.join("home/config/Thunar/uca.xml")).unwrap();
    assert!(thunar.contains("start %f"));
    // A current install is left alone.
    let before = fs::metadata(deb.nemo()).unwrap().modified().unwrap();
    assert!(services.ensure_explorer().unwrap().installed);
    assert_eq!(
        fs::metadata(deb.nemo()).unwrap().modified().unwrap(),
        before
    );
    // Explicit removal (not reachable from the always-on API) cleans up owned files.
    services
        .perform_user_action(SystemAction::SetExplorer {
            installed: false,
            scope: SystemScope::User,
        })
        .unwrap();
    assert!(!deb.nemo().exists());
    assert!(launcher.exists(), "the package owns its launcher");
}

#[test]
fn deb_repairs_stale_owned_menus_and_keeps_foreign_ones() {
    let deb = Deb::new();
    fs::create_dir_all(deb.nemo().parent().unwrap()).unwrap();
    // An older owned entry, as 0.9 left it after a template change.
    fs::write(deb.nemo(), "# Shellfox/linux-v1\n[Nemo Action]\nExec=old\n").unwrap();
    let services = deb.services();
    assert!(services.ensure_explorer().unwrap().installed);
    assert!(!fs::read_to_string(deb.nemo()).unwrap().contains("Exec=old"));
    fs::write(deb.nemo(), "[Nemo Action]\nExec=someone-else\n").unwrap();
    assert_eq!(
        services.ensure_explorer().unwrap_err().code,
        ErrorCode::AuthFailed
    );
    assert_eq!(
        fs::read_to_string(deb.nemo()).unwrap(),
        "[Nemo Action]\nExec=someone-else\n"
    );
}

#[test]
fn deb_without_a_file_manager_or_launcher_reports_why() {
    let deb = Deb::new();
    for program in ["nemo", "thunar"] {
        fs::remove_file(deb.root.join("programs").join(program)).unwrap();
    }
    let error = deb.services().ensure_explorer().unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(error.message.contains("No supported file manager"));
    fs::write(
        deb.root.join("usr/bin/shellfox"),
        "#!/bin/sh\necho foreign\n",
    )
    .unwrap();
    assert!(deb.services().ensure_cli().is_err());
    assert!(!deb.root.join("home/data").exists());
}
