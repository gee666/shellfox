#![cfg(feature = "test-helper")]
use shellfox_transport::*;
mod support;
use support::{executable, Scratch};

async fn fixture() -> (Scratch, PermissionVerifier, IsolatedStateDirectory) {
    let scratch = Scratch::new();
    let verifier = PermissionVerifier::new(executable());
    verifier
        .protect(&scratch.0, PrivatePathKind::Directory)
        .await
        .unwrap();
    let root = IsolatedStateDirectory::create_new_private(scratch.0.join("node"), &verifier)
        .await
        .unwrap();
    (scratch, verifier, root)
}
#[tokio::test]
async fn marked_private_root_reopens_without_reading_or_replacing_node_credentials() {
    let (_scratch, verifier, root) = fixture().await;
    let path = root.path().to_owned();
    let marker = path.join(IsolatedStateDirectory::MARKER_FILE);
    let bytes = std::fs::read(&marker).unwrap();
    assert!(bytes.len() <= 256);
    let credentials = path.join("tailscaled.state");
    let opaque = b"opaque synthetic state, deliberately not valid node JSON";
    std::fs::write(&credentials, opaque).unwrap();
    let clone = root.clone();
    drop(root);
    assert_eq!(clone.path(), path);
    drop(clone);
    let reopened = IsolatedStateDirectory::open_existing(path.clone(), &verifier)
        .await
        .unwrap();
    assert_eq!(reopened.path(), path);
    assert_eq!(std::fs::read(&credentials).unwrap(), opaque);
    assert_eq!(std::fs::read(marker).unwrap(), bytes);
    assert!(IsolatedStateDirectory::create_new(path).is_err());
    assert_eq!(std::fs::read(credentials).unwrap(), opaque);
    drop(reopened);
}
#[tokio::test]
async fn unmarked_existing_folders_are_refused_not_adopted_or_overwritten() {
    let (scratch, verifier, root) = fixture().await;
    drop(root);
    let unmarked = scratch.0.join("unmarked-old-node");
    std::fs::create_dir(&unmarked).unwrap();
    let credentials = unmarked.join("tailscaled.state");
    std::fs::write(&credentials, b"existing opaque fixture").unwrap();
    let result = IsolatedStateDirectory::open_existing(unmarked.clone(), &verifier).await;
    assert_eq!(result.unwrap_err(), HelperError::StateUnmarked);
    assert!(HelperError::StateUnmarked
        .to_string()
        .contains("not adopted"));
    assert!(IsolatedStateDirectory::create_new(unmarked.clone()).is_err());
    assert!(
        IsolatedStateDirectory::create_new_private(unmarked.clone(), &verifier)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(credentials).unwrap(),
        b"existing opaque fixture"
    );
    assert!(!unmarked.join(IsolatedStateDirectory::MARKER_FILE).exists());
}
#[tokio::test]
async fn marker_is_bounded_strict_versioned_and_bound_to_the_fixed_root_path() {
    let (scratch, verifier, root) = fixture().await;
    let path = root.path().to_owned();
    let marker = path.join(IsolatedStateDirectory::MARKER_FILE);
    let original = std::fs::read(&marker).unwrap();
    drop(root);
    let unknown_version = String::from_utf8(original.clone())
        .unwrap()
        .replace("\"version\":1", "\"version\":2")
        .into_bytes();
    let mut trailing = original.clone();
    trailing.extend_from_slice(b" ");
    for bytes in [b"{}".to_vec(), unknown_version, trailing, vec![b'x'; 257]] {
        std::fs::write(&marker, &bytes).unwrap();
        assert_eq!(
            IsolatedStateDirectory::open_existing(path.clone(), &verifier)
                .await
                .unwrap_err(),
            HelperError::StateMarkerInvalid
        );
        assert_eq!(std::fs::read(&marker).unwrap(), bytes);
    }
    std::fs::write(&marker, &original).unwrap();
    let other = IsolatedStateDirectory::create_new_private(scratch.0.join("other"), &verifier)
        .await
        .unwrap();
    let other_path = other.path().to_owned();
    drop(other);
    std::fs::write(
        other_path.join(IsolatedStateDirectory::MARKER_FILE),
        original,
    )
    .unwrap();
    assert_eq!(
        IsolatedStateDirectory::open_existing(other_path, &verifier)
            .await
            .unwrap_err(),
        HelperError::StateMarkerInvalid
    );
}
#[cfg(unix)]
#[tokio::test]
async fn unix_links_and_unsafe_marker_modes_refuse_without_permission_repair() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (scratch, verifier, root) = fixture().await;
    let path = root.path().to_owned();
    let marker = path.join(IsolatedStateDirectory::MARKER_FILE);
    drop(root);
    let alias = scratch.0.join("alias");
    symlink(&path, &alias).unwrap();
    assert!(IsolatedStateDirectory::open_existing(alias, &verifier)
        .await
        .is_err());
    std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        IsolatedStateDirectory::open_existing(path.clone(), &verifier)
            .await
            .unwrap_err(),
        HelperError::PermissionDenied
    );
    assert_eq!(
        std::fs::metadata(&marker).unwrap().permissions().mode() & 0o777,
        0o644
    );
    std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = scratch.0.join("linked-marker");
    std::fs::hard_link(&marker, &link).unwrap();
    assert_eq!(
        IsolatedStateDirectory::open_existing(path, &verifier)
            .await
            .unwrap_err(),
        HelperError::PermissionDenied
    );
}
