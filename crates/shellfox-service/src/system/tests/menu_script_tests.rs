//! The Files and Caja Scripts entries are plain sh.
use super::super::linux_templates::script;
use std::path::Path;

#[test]
fn menu_scripts_need_only_sh_and_quote_the_launcher() {
    let launcher = Path::new(if cfg!(windows) {
        r"C:\opt\it's here\shellfox"
    } else {
        "/opt/it's here/shellfox"
    });
    let quoted = launcher.to_str().unwrap().replace('\'', r"'\''");
    for (caja, prefix) in [(false, "NAUTILUS"), (true, "CAJA")] {
        let text = script(launcher, caja).unwrap();
        assert!(text.starts_with("#!/bin/sh\n# Shellfox/linux-v1\n"));
        assert!(!text.contains("python"));
        assert!(text.contains(&format!("${{{prefix}_SCRIPT_CURRENT_URI:-}}")));
        assert!(text.contains(&format!("exec '{quoted}' start \"$folder\"\n")));
    }
    assert!(script(Path::new("relative/shellfox"), false).is_err());
}

/// Runs the generated script the way Files does: in the viewed folder, with
/// the selection as arguments relative to it. Linux only: macOS file systems
/// reject non-UTF-8 names.
#[cfg(target_os = "linux")]
#[test]
fn menu_scripts_open_the_viewed_or_selected_local_folder() {
    use std::{
        ffi::OsStr,
        fs,
        os::unix::{ffi::OsStrExt, fs::PermissionsExt},
        process::Command,
    };
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    fs::create_dir_all(project.join("tmp")).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("menu-script-")
        .tempdir_in(project.join("tmp"))
        .unwrap();
    let root = temp.path();
    let out = root.join("out");
    let launcher = root.join("launcher");
    fs::write(&launcher, "#!/bin/sh\nprintf '%s\\0' \"$@\" > \"$OUT\"\n").unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
    let entry = root.join("Open in Shellfox");
    fs::write(&entry, script(&launcher, false).unwrap()).unwrap();
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
    let odd = OsStr::from_bytes(b"caf\xe9 dir");
    fs::create_dir_all(root.join("with space").join(odd)).unwrap();
    // sh reports the physical working directory.
    let viewed = fs::canonicalize(root.join("with space")).unwrap();
    fs::write(viewed.join("file"), b"").unwrap();
    let run = |uri: &str, args: &[&OsStr]| {
        let _ = fs::remove_file(&out);
        let status = Command::new(&entry)
            .args(args)
            .current_dir(&viewed)
            .env("OUT", &out)
            .env("NAUTILUS_SCRIPT_CURRENT_URI", uri)
            .status()
            .unwrap();
        status.success().then(|| fs::read(&out).unwrap())
    };
    let expected = |folder: &Path| {
        let mut bytes = b"start\0".to_vec();
        bytes.extend_from_slice(folder.as_os_str().as_bytes());
        bytes.push(0);
        bytes
    };
    let uri = "file:///ignored%20for%20local";
    assert_eq!(run(uri, &[]), Some(expected(&viewed)));
    assert_eq!(run(uri, &[odd]), Some(expected(&viewed.join(odd))));
    let absolute = viewed.join(odd);
    assert_eq!(
        run("FILE://localhost/x", &[absolute.as_os_str()]),
        Some(expected(&absolute))
    );
    assert_eq!(run(uri, &[OsStr::new("file")]), None);
    assert_eq!(run(uri, &[odd, odd]), None);
    assert_eq!(run("sftp://host/dir", &[]), None);
    assert_eq!(run("", &[]), None);
}
