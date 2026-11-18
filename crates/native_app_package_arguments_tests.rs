use super::*;
fn paths() -> (PathBuf, PathBuf) {
    if cfg!(windows) {
        (
            PathBuf::from(r"D:\producer-stable\package-manifest.json"),
            PathBuf::from(r"D:\host-data"),
        )
    } else {
        (
            PathBuf::from("/producer-stable/package-manifest.json"),
            PathBuf::from("/host-data"),
        )
    }
}
#[test]
fn absent_package_is_not_discovered_and_paired_selection_is_planning_only() {
    let (path, data) = paths();
    for flag in ["--package-manifest", "--package-manifest-sha256"] {
        assert!(super::super::ApplicationArguments::is_flag(
            std::ffi::OsStr::new(flag)
        ));
    }
    assert!(PackageArguments::default().finish(&data).unwrap().is_none());
    let mut args = PackageArguments::default();
    args.push("--package-manifest", path.clone().into_os_string())
        .unwrap();
    args.push("--package-manifest-sha256", "aF".repeat(32).into())
        .unwrap();
    let selected = args.finish(&data).unwrap().unwrap();
    assert_eq!(selected.path, path);
    assert_eq!(selected.expected_digest, [0xaf; 32]);
    // No resource-opening method called; this is not installed-package evidence.
}
#[test]
fn missing_pair_duplicate_or_non_external_digest_syntax_is_refused() {
    let (path, data) = paths();
    let mut args = PackageArguments::default();
    args.push("--package-manifest", path.into_os_string())
        .unwrap();
    assert!(args.finish(&data).is_err());
    let mut args = PackageArguments::default();
    args.push("--package-manifest-sha256", "a".repeat(64).into())
        .unwrap();
    assert!(
        args.push("--package-manifest-sha256", "b".repeat(64).into())
            .is_err()
    );
    assert!(args.finish(&data).is_err());
    for value in ["".into(), "a".repeat(63), "a".repeat(65), "g".repeat(64)] {
        assert!(
            PackageArguments::default()
                .push("--package-manifest-sha256", value.into())
                .is_err()
        );
    }
}
#[test]
fn paths_cannot_be_relative_traversing_or_overlap_mutable_host_data() {
    assert!(
        PackageArguments::default()
            .push("--package-manifest", "package-manifest.json".into())
            .is_err()
    );
    let (path, _) = paths();
    let root = path.parent().unwrap();
    assert!(
        PackageArguments::default()
            .push(
                "--package-manifest",
                root.join("../escape/package-manifest.json")
                    .into_os_string()
            )
            .is_err()
    );
    for candidate in [
        root.join("package-manifest.json"),
        root.join("../escape/package-manifest.json"),
    ] {
        let mut args = PackageArguments::default();
        if args
            .push("--package-manifest", candidate.into_os_string())
            .is_ok()
        {
            args.push("--package-manifest-sha256", "a".repeat(64).into())
                .unwrap();
            assert!(args.finish(&root.join("mutable-data")).is_err());
        }
    }
}
