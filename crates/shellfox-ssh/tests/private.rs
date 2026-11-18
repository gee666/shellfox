mod common;
use common::*;
use shellfox_ssh::{Error, PrivateDirectory, ProfileStore};
#[cfg(windows)]
use shellfox_ssh::{ProcessOutput, ProcessRequest, ProcessRunner, SystemProcessRunner};
use std::fs;
#[cfg(windows)]
use std::sync::Arc;
#[test]
fn hardlinks_and_non_files_are_rejected_without_overwrite() {
    let fixture = Fixture::new();
    let store = ProfileStore::new(fixture.directory.clone());
    store.save(profile("one")).unwrap();
    let file = fixture.path.join("ssh-profiles.json");
    let alias = fixture.path.join("alias.json");
    fs::hard_link(&file, &alias).unwrap();
    let original = fs::read(&alias).unwrap();
    assert_eq!(store.list().unwrap_err(), Error::Permissions);
    assert_eq!(store.save(profile("two")).unwrap_err(), Error::Permissions);
    assert_eq!(fs::read(&alias).unwrap(), original);
    fs::remove_file(alias).unwrap();
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    assert_eq!(store.list().unwrap_err(), Error::Permissions);
}
#[cfg(unix)]
#[test]
fn unix_ownership_modes_and_symlinks_fail_closed() {
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    let fixture = Fixture::new();
    let store = ProfileStore::new(fixture.directory.clone());
    store.save(profile("one")).unwrap();
    let file = fixture.path.join("ssh-profiles.json");
    assert_eq!(fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&fixture.path).unwrap().mode() & 0o777, 0o700);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(store.list().unwrap_err(), Error::Permissions);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = fixture.path.join("other.json");
    fs::rename(&file, &alias).unwrap();
    symlink(&alias, &file).unwrap();
    assert!(store.list().is_err());
    assert!(store.save(profile("two")).is_err());
    let linked = fixture.temp.path().join("linked");
    symlink(&fixture.path, &linked).unwrap();
    assert!(PrivateDirectory::open(linked, policy()).is_err());
    fs::set_permissions(&fixture.path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(PrivateDirectory::open(&fixture.path, policy()).is_err());
    fs::set_permissions(&fixture.path, fs::Permissions::from_mode(0o700)).unwrap();
}
#[cfg(windows)]
struct RejectStages;
#[cfg(windows)]
impl ProcessRunner for RejectStages {
    fn run(&self, request: &ProcessRequest) -> shellfox_ssh::Result<ProcessOutput> {
        if request
            .args
            .iter()
            .any(|a| a.to_string_lossy().ends_with(".tmp"))
        {
            return Err(Error::PermissionsHelper);
        }
        SystemProcessRunner.run(request)
    }
}
#[cfg(windows)]
#[test]
fn windows_acl_helper_is_required_and_failed_stage_never_replaces_old_data() {
    let fixture = Fixture::new();
    let store = ProfileStore::new(fixture.directory.clone());
    store.save(profile("one")).unwrap();
    let file = fixture.path.join("ssh-profiles.json");
    let original = fs::read(&file).unwrap();
    assert!(matches!(
        PrivateDirectory::open(&fixture.path, shellfox_ssh::StoragePolicy::default()),
        Err(Error::PermissionsHelper)
    ));
    let rejected = PrivateDirectory::open(
        &fixture.path,
        policy().with_process_runner(Arc::new(RejectStages)),
    )
    .unwrap();
    let rejected = ProfileStore::new(Arc::new(rejected));
    assert_eq!(
        rejected.save(profile("two")).unwrap_err(),
        Error::PermissionsHelper
    );
    assert_eq!(fs::read(file).unwrap(), original);
    assert_eq!(
        fs::read_dir(&fixture.path)
            .unwrap()
            .filter(|p| p
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "tmp"))
            .count(),
        0
    );
}
