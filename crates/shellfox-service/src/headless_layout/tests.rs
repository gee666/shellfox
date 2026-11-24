use super::*;

fn package(root: &Path, version: &str) {
    fs::create_dir_all(root).unwrap();
    let build = format!(
        r#"{{"product":"headless","version":"{version}","platform":"{}","arch":"{}"}}"#,
        platform(),
        arch()
    );
    fs::write(root.join("build.json"), build).unwrap();
}

/// Same layout `shellfox update` (0.9.x and Rust) creates beside the anchor.
fn version_dir(parent: &Path, version: &str) -> PathBuf {
    let root = parent
        .join(format!(
            "shellfox-headless-{version}-{}-{}-Ab12cD",
            platform(),
            arch()
        ))
        .join("installation")
        .join(root_name());
    package(&root, version);
    normalize(&root)
}

#[test]
fn activation_selects_a_version_for_the_anchor_and_every_version() {
    let temp = tempfile::tempdir().unwrap();
    let parent = normalize(temp.path());
    let anchor = parent.join(root_name());
    package(&anchor, "0.9.15");
    assert!(active(&anchor).unwrap().is_none());
    let first = version_dir(&parent, "0.10.0");
    activate(&anchor, &first).unwrap();
    assert_eq!(active(&anchor).unwrap(), Some(first.clone()));
    assert_eq!(anchor_of(&first), anchor);
    assert!(
        active(&first).unwrap().is_none(),
        "the selected version never forwards to itself"
    );
    // A Rust-to-Rust update from the selected version re-points the same anchor.
    let second = version_dir(&parent, "0.10.1");
    activate(&anchor_of(&first), &second).unwrap();
    assert_eq!(active(&first).unwrap(), Some(second.clone()));
    assert_eq!(active(&anchor).unwrap(), Some(second));
    let pointer: serde_json::Value =
        serde_json::from_slice(&fs::read(anchor.join(POINTER)).unwrap()).unwrap();
    assert_eq!(pointer["version"], 1);
}

fn anchor_of(root: &Path) -> PathBuf {
    super::anchor(root).unwrap()
}

#[test]
fn versions_outside_the_anchor_parent_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let parent = normalize(temp.path());
    let anchor = parent.join("stable").join(root_name());
    package(&anchor, "0.9.15");
    let stray = version_dir(&parent.join("elsewhere"), "0.10.0");
    assert!(activate(&anchor, &stray).is_err());
    assert!(!anchor.join(POINTER).exists());
    for name in [
        "shellfox-headless-0.10-linux-x64-a",
        "shellfox-headless-0.10.0-plan9-x64-a",
        "shellfox-headless-0.10.0-linux-x64-",
        "shellfox-headless-0.10.0-linux-x64-a_b",
    ] {
        assert!(!version_directory_name(name), "{name}");
    }
    assert!(version_directory_name(
        "shellfox-headless-0.10.0-win32-arm64-Q1w2E3"
    ));
}

#[test]
fn pointers_from_another_anchor_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let parent = normalize(temp.path());
    let (one, two) = (parent.join(root_name()), parent.join("other"));
    package(&one, "0.9.15");
    package(&two, "0.9.15");
    let version = version_dir(&parent, "0.10.0");
    activate(&one, &version).unwrap();
    fs::copy(one.join(POINTER), two.join(POINTER)).unwrap();
    assert!(active(&two).is_err());
}
