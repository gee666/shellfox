use super::super::*;
use super::support::Environment;
use shellfox_protocol::ErrorCode;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// An extracted archive in a folder whose name needs quoting, and a HOME with
/// Nemo and Caja on PATH, under tmp/.
struct Portable {
    _temp: tempfile::TempDir,
    root: PathBuf,
    app: PathBuf,
    home: PathBuf,
}
impl Portable {
    fn new() -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        fs::create_dir_all(project.join("tmp")).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("portable-menus-")
            .tempdir_in(project.join("tmp"))
            .unwrap();
        let root = temp.path().to_owned();
        let app = root.join("my 'apps'").join("shellfox-0.10.0-linux-x64");
        let home = root.join("home");
        for dir in [&app, &home, &root.join("programs")] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(app.join("shellfox"), b"synthetic desktop").unwrap();
        fs::write(app.join("shellfox-cli"), b"synthetic cli").unwrap();
        for program in ["nemo", "caja"] {
            let path = root.join("programs").join(program);
            fs::write(&path, b"").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        Self {
            _temp: temp,
            root,
            app,
            home,
        }
    }
    fn environment(&self) -> Arc<Environment> {
        Arc::new(Environment {
            platform: SystemPlatform::Linux,
            home: self.home.clone(),
            variables: [
                ("PATH".into(), self.root.join("programs").into_os_string()),
                (
                    "XDG_DATA_DIRS".into(),
                    self.root.join("share").into_os_string(),
                ),
            ]
            .into_iter()
            .collect(),
        })
    }
    fn services(&self) -> HostSystemServices {
        let portable =
            PortableLinux::at(self.app.clone(), self.launcher().parent().unwrap().into());
        HostSystemServices::new(Arc::new(portable.backend(self.environment())))
    }
    fn launcher(&self) -> PathBuf {
        self.home.join(".local/bin").join("shellfox")
    }
}

#[test]
fn portable_writes_the_launcher_and_menus_that_call_it() {
    let portable = Portable::new();
    let services = portable.services();
    assert!(services.ensure_cli().unwrap().installed);
    let launcher = fs::read_to_string(portable.launcher()).unwrap();
    let quoted = |name: &str| {
        let path = portable.app.join(name);
        format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
    };
    assert!(launcher.starts_with("#!/bin/sh\n# Shellfox/linux-v1\n"));
    assert!(launcher.contains(&format!("exe={}\n", quoted("shellfox"))));
    assert!(launcher.contains(&format!("cli={}\n", quoted("shellfox-cli"))));
    assert!(launcher.contains(&posix_router()));
    assert!(services.ensure_explorer().unwrap().installed);
    let target = portable.launcher();
    let nemo = fs::read_to_string(
        portable
            .home
            .join(".local/share/nemo/actions/shellfox.nemo_action"),
    )
    .unwrap();
    // Exec quoting escapes backslashes twice; only Windows test paths have them.
    let exec = format!("Exec=\"{}\" start %F", target.to_str().unwrap());
    let unescape = |line: &str| line.replace(r"\\", r"\").replace(r"\\", r"\");
    assert!(nemo.lines().any(|line| unescape(line) == exec));
    let caja =
        fs::read_to_string(portable.home.join(".config/caja/scripts/Open in Shellfox")).unwrap();
    assert!(caja.contains(&format!("exec '{}' start", target.to_str().unwrap())));
    // A current install is left alone.
    let before = fs::metadata(&target).unwrap().modified().unwrap();
    assert!(services.ensure_cli().unwrap().installed);
    assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), before);
}

#[test]
fn portable_replaces_its_old_launcher_and_keeps_a_foreign_one() {
    let portable = Portable::new();
    fs::create_dir_all(portable.launcher().parent().unwrap()).unwrap();
    // 0.9's portable launcher carried the same owner line.
    fs::write(
        portable.launcher(),
        "#!/bin/sh\n# Shellfox/linux-v1\nexport SHELLFOX_CLI_LAUNCH=old\n",
    )
    .unwrap();
    assert!(portable.services().ensure_cli().unwrap().installed);
    assert!(!fs::read_to_string(portable.launcher())
        .unwrap()
        .contains("SHELLFOX_CLI_LAUNCH"));
    fs::write(portable.launcher(), "#!/bin/sh\necho mine\n").unwrap();
    assert_eq!(
        portable.services().ensure_cli().unwrap_err().code,
        ErrorCode::AuthFailed
    );
    assert_eq!(
        fs::read_to_string(portable.launcher()).unwrap(),
        "#!/bin/sh\necho mine\n"
    );
}

#[test]
fn only_a_complete_extracted_folder_is_portable() {
    let portable = Portable::new();
    let environment = portable.environment();
    let found = PortableLinux::current(&portable.app.join("shellfox"), &*environment).unwrap();
    assert_eq!(
        found.cli_plan(SystemScope::User).unwrap().directory,
        portable.home.join(".local/bin")
    );
    assert!(found.cli_plan(SystemScope::System).is_err());
    assert!(PortableLinux::current(&portable.app.join("shellfox-cli"), &*environment).is_none());
    fs::remove_file(portable.app.join("shellfox-cli")).unwrap();
    assert!(PortableLinux::current(&portable.app.join("shellfox"), &*environment).is_none());
    let error = portable.services().ensure_cli().unwrap_err();
    assert_eq!(error.code, ErrorCode::NativeUnavailable);
    assert!(!portable.launcher().exists());
}

/// Runs the real launcher against stub binaries that record their arguments.
#[cfg(unix)]
#[test]
fn portable_launcher_routes_desktop_verbs_and_cli_commands() {
    use std::os::unix::fs::PermissionsExt;
    let portable = Portable::new();
    let out = portable.root.join("out");
    for (name, tag) in [("shellfox", "desktop"), ("shellfox-cli", "cli")] {
        let path = portable.app.join(name);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '{tag}:%s\\n' \"$@\" > \"$OUT.tmp-{tag}\"\nmv \"$OUT.tmp-{tag}\" \"$OUT.{tag}\"\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    portable.services().ensure_cli().unwrap();
    let run = |args: &[&str]| {
        let status = std::process::Command::new(portable.launcher())
            .args(args)
            .env("OUT", &out)
            .status()
            .unwrap();
        assert!(status.success());
    };
    run(&["session", "list"]);
    assert_eq!(
        fs::read_to_string(out.with_extension("cli")).unwrap(),
        "cli:session\ncli:list\n"
    );
    run(&["start", "/a b"]);
    let desktop = out.with_extension("desktop");
    // The launcher starts the desktop in the background and returns.
    for _ in 0..100 {
        if desktop.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(desktop).unwrap(),
        "desktop:start\ndesktop:/a b\n"
    );
}
