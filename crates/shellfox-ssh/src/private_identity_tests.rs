use super::super::StoragePolicy;
use super::*;
use std::fs;
#[cfg(windows)]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
fn fixture() -> Result<(tempfile::TempDir, PrivateDirectory, PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    fs::create_dir_all(&root)?;
    let temp = tempfile::Builder::new()
        .prefix("external-identity-")
        .tempdir_in(root)?;
    let root = fs::canonicalize(temp.path())?;
    #[cfg(windows)]
    let root = PathBuf::from(
        root.to_str()
            .ok_or(Error::Validation)?
            .strip_prefix(r"\\?\")
            .unwrap_or(root.to_str().ok_or(Error::Validation)?),
    );
    #[cfg(windows)]
    let policy = StoragePolicy::with_windows_helper(
        dev_helper::helper()
            .expect("development Tailscale helper")
            .0,
    );
    #[cfg(not(windows))]
    let policy = StoragePolicy::default();
    let store = PrivateDirectory::open(root.join("private-store"), policy.clone())?;
    let ordinary = root.join("ordinary-parent");
    fs::create_dir(&ordinary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&ordinary, fs::Permissions::from_mode(0o755))?;
    }
    // Construct a synthetic fixture file with the existing private-store creator,
    // then move it into an ordinary parent without altering its FILE privacy.
    store.atomic_write("selected-key", b"synthetic identity snapshot", 1024)?;
    let key = ordinary.join("selected-key");
    fs::rename(store.path().join("selected-key"), &key)?;
    assert!(
        PrivateDirectory::open(&ordinary, policy).is_err(),
        "ordinary parent was promoted to private-store authority"
    );
    Ok((temp, store, key))
}
#[test]
fn private_identity_in_ordinary_parent_reads_the_checked_snapshot() -> Result<()> {
    let (_temp, store, key) = fixture()?;
    assert_eq!(
        &store.read_key_file(&key)?[..],
        b"synthetic identity snapshot"
    );
    Ok(())
}
#[test]
fn nonprivate_file_and_hardlinked_selection_remain_denied() -> Result<()> {
    let (_temp, store, key) = fixture()?;
    let unsafe_file = key.with_file_name("unsafe-key");
    fs::write(&unsafe_file, b"not a private identity")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unsafe_file, fs::Permissions::from_mode(0o644))?;
    }
    assert!(store.read_key_file(&unsafe_file).is_err());
    let alias = key.with_file_name("hardlink");
    fs::hard_link(&key, &alias)?;
    assert!(store.read_key_file(&key).is_err());
    Ok(())
}
#[cfg(windows)]
#[test]
fn windows_spelling_is_native_without_device_or_store_escape() -> Result<()> {
    assert_eq!(
        native_helper_path(Path::new("C:/Users/user/.ssh/id"))?,
        PathBuf::from(r"C:\Users\user\.ssh\id")
    );
    for path in [
        r"\\?\C:\key",
        r"\\.\C:\key",
        r"C:\folder\..\key",
        r"C:\key:stream",
        r"C:\trailing.\key",
    ] {
        assert!(native_helper_path(Path::new(path)).is_err());
    }
    Ok(())
}
#[cfg(windows)]
#[test]
fn windows_identity_pins_deny_write_file_and_parent_replacement() -> Result<()> {
    let (_temp, store, key) = fixture()?;
    let checked = store.checked_identity(&key)?;
    assert!(fs::OpenOptions::new().write(true).open(&key).is_err());
    assert!(fs::rename(&key, key.with_file_name("moved-key")).is_err());
    assert!(fs::remove_file(&key).is_err());
    assert!(fs::rename(
        key.parent().unwrap(),
        key.parent().unwrap().with_extension("moved")
    )
    .is_err());
    checked.current()?;
    Ok(())
}
#[cfg(unix)]
#[test]
fn unix_replacement_parent_change_and_mutation_are_fenced() -> Result<()> {
    use std::os::unix::{
        fs::{symlink, PermissionsExt},
        io::AsRawFd,
    };
    let (_temp, store, key) = fixture()?;
    let mut checked = store.checked_identity(&key)?;
    let original = checked.file.as_raw_fd();
    fs::rename(&key, key.with_extension("old"))?;
    fs::write(&key, b"substituted selection")?;
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600))?;
    assert!(checked.current().is_err());
    let mut bytes = Vec::new();
    checked.file.read_to_end(&mut bytes)?;
    assert_eq!(checked.file.as_raw_fd(), original);
    assert_eq!(bytes, b"synthetic identity snapshot");
    let selection = store.checked_identity(&key)?;
    fs::write(&key, b"changed same inode")?;
    assert!(selection.current().is_err());
    let selection = store.checked_identity(&key)?;
    let parent = key.parent().unwrap();
    fs::rename(parent, parent.with_extension("old"))?;
    fs::create_dir(parent)?;
    assert!(selection.current().is_err());
    let link = store.path().join("symlink-key");
    symlink(parent.with_extension("old").join("selected-key"), &link)?;
    assert!(store.read_key_file(&link).is_err());
    Ok(())
}
