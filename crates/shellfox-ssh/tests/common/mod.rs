#![allow(dead_code)]
use shellfox_ssh::{PasswordUpdate, PrivateDirectory, ProfileInput, StoragePolicy};
use std::{path::PathBuf, sync::Arc};
use tempfile::TempDir;
use uuid::Uuid;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
pub fn policy() -> StoragePolicy {
    if cfg!(windows) {
        let helper = std::env::var_os("SHELLFOX_SSH_TEST_PERMISSIONS_HELPER")
            .map(PathBuf::from)
            .unwrap_or_else(|| root().join("tmp/dev-helper/shellfox-tailscale.exe"));
        assert!(
            helper.is_file(),
            "Build/configure the real private-permissions helper, never bypass ACL checks"
        );
        StoragePolicy::with_windows_helper(helper)
    } else {
        StoragePolicy::default()
    }
}
pub struct Fixture {
    pub directory: Arc<PrivateDirectory>,
    pub path: PathBuf,
    pub temp: TempDir,
}
impl Fixture {
    pub fn new() -> Self {
        std::fs::create_dir_all(root().join("tmp")).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("ssh-rust-")
            .tempdir_in(root().join("tmp"))
            .unwrap();
        let path = temp.path().join("private");
        let directory = Arc::new(PrivateDirectory::open(&path, policy()).unwrap());
        Self {
            temp,
            path,
            directory,
        }
    }
}
pub fn profile(name: &str) -> ProfileInput {
    ProfileInput {
        id: Uuid::new_v4(),
        name: name.into(),
        host: "example.test".into(),
        port: 22,
        user: "deploy".into(),
        password: PasswordUpdate::Keep,
        key_file: None,
        remote_cwd: Some("/srv".into()),
    }
}
pub fn public_key() -> Vec<u8> {
    ssh_key::PrivateKey::random(&mut rand::rngs::OsRng, ssh_key::Algorithm::Ed25519)
        .unwrap()
        .public_key()
        .to_bytes()
        .unwrap()
}
