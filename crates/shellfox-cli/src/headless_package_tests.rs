use super::*;
use crate::headless_layout::{arch, platform};

/// A complete package directory: the Rust payload plus the 0.9.x names.
pub(crate) fn fake_package(root: &Path, version: &str) {
    for name in required_files(platform()) {
        let file = root.join(&name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let node = if platform() == "win32" {
        "bin/node.exe"
    } else {
        "bin/node"
    };
    fs::write(root.join("runtime.json"), format!(r#"{{"version":1,"node":"{node}","entry":"app/main.cjs","resources":"resources","runAsNode":false,"headless":true}}"#)).unwrap();
    fs::write(root.join("build.json"), format!(r#"{{"product":"headless","version":"{version}","platform":"{}","arch":"{}","commit":"abc"}}"#, platform(), arch())).unwrap();
}

#[test]
fn complete_packages_pass_and_incomplete_or_foreign_ones_fail() {
    let temp = tempfile::tempdir().unwrap();
    fake_package(temp.path(), "0.10.0");
    validate(temp.path(), "0.10.0", platform(), arch()).unwrap();
    assert!(validate(temp.path(), "0.10.1", platform(), arch()).is_err());
    fs::write(temp.path().join("app/main.cjs"), b"").unwrap();
    assert!(validate(temp.path(), "0.10.0", platform(), arch()).is_err());
    fs::write(temp.path().join("app/main.cjs"), b"x").unwrap();
    fs::write(temp.path().join("runtime.json"), br#"{"version":1,"node":"bin/other","entry":"app/main.cjs","resources":"resources","runAsNode":false,"headless":true}"#).unwrap();
    assert!(validate(temp.path(), "0.10.0", platform(), arch()).is_err());
}

#[test]
fn archive_listings_allow_only_plain_entries_under_one_root() {
    let ok = "r/\nr/bin/\nr/bin/a\n";
    let details = "drwx r/\ndrwx r/bin/\n-rw- r/bin/a\n";
    validate_listing(ok, details, "r").unwrap();
    for (names, details) in [
        ("r/../x\n", "-rw- x\n"),
        ("other/a\n", "-rw- a\n"),
        ("r/a\n", "lrwx a -> /etc\n"),
        ("r/CON.txt\n", "-rw- c\n"),
        ("r/a\nr/A\n", "-rw- a\n-rw- A\n"),
        ("r/a.\n", "-rw- a\n"),
        ("r/a\n", ""),
    ] {
        assert!(validate_listing(names, details, "r").is_err(), "{names:?}");
    }
}
