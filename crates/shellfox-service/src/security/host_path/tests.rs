use super::*;
pub(super) fn scratch() -> tempfile::TempDir {
    let root =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp")).unwrap();
    tempfile::Builder::new()
        .prefix("native-host-path-")
        .tempdir_in(root)
        .unwrap()
}
#[test]
fn missing_root_inspection_is_pure_and_dot_paths_are_rejected() {
    let data = scratch();
    let root = data.path().join("new").join("host");
    let plan = HostRoot::inspect(&root).unwrap();
    assert!(!plan.exists() && !root.exists() && !data.path().join("new").exists());
    // PathBuf::push normalizes dots in verbatim Windows paths, so preserve the
    // original input spelling rather than constructing this fixture with join.
    let dotted = PathBuf::from(format!(
        "{}{}..{}escape",
        data.path().display(),
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    ));
    assert!(HostRoot::inspect(&dotted).is_err());
    assert!(!data.path().join("escape").exists());
}
#[test]
fn replaced_original_anchor_cannot_receive_a_created_child() {
    let data = scratch();
    let parent = data.path().join("parent");
    std::fs::create_dir(&parent).unwrap();
    let mut plan = HostRoot::inspect(&parent.join("new-host")).unwrap();
    let renamed = data.path().join("old-parent");
    match std::fs::rename(&parent, &renamed) {
        Ok(()) => {
            std::fs::create_dir(&parent).unwrap();
            assert!(plan.create_missing().is_err());
            assert!(!parent.join("new-host").exists() && !renamed.join("new-host").exists());
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || (cfg!(windows) && error.raw_os_error() == Some(32)) =>
        {
            assert!(!parent.join("new-host").exists()); // OS itself retained the original anchor.
        }
        Err(error) => panic!("unexpected owned rename error: {error}"),
    }
}
fn alias(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return;
        }
        let command =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe");
        let output = std::process::Command::new(command)
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "owned junction fixture unavailable"
        );
    }
}
#[test]
fn original_root_and_ancestor_aliases_are_rejected_without_creating_descendants() {
    let data = scratch();
    let real = data.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = data.path().join("alias");
    alias(&real, &link);
    assert!(HostRoot::inspect(&link).is_err());
    assert!(HostRoot::inspect(&link.join("new").join("host")).is_err());
    assert!(!real.join("new").exists() && !real.join("host").exists());
}
