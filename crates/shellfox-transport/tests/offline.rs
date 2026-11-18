#![cfg(feature = "test-helper")]
use shellfox_transport::*;
use std::path::PathBuf;
mod support;
use support::{executable, Scratch};

#[tokio::test]
async fn retained_offline_modes_have_owned_lifetimes() {
    let scratch = Scratch::new();
    let verifier = PermissionVerifier::new(executable());
    verifier
        .protect(&scratch.0, PrivatePathKind::Directory)
        .await
        .unwrap();
    verifier
        .check(&scratch.0, PrivatePathKind::Directory)
        .await
        .unwrap();
    assert_eq!(
        verifier
            .check(&PathBuf::from("relative"), PrivatePathKind::File)
            .await,
        Err(HelperError::InvalidRequest)
    );
    let ownership = HostOwnership::acquire(&executable(), &scratch.0)
        .await
        .unwrap()
        .unwrap();
    let lost = ownership.lost();
    assert!(ownership.is_alive());
    ownership.shutdown().await.unwrap();
    assert!(lost.is_cancelled());
    let ownership = HostOwnership::acquire(&executable(), &scratch.0)
        .await
        .unwrap()
        .unwrap();
    let lost = ownership.lost();
    drop(ownership);
    tokio::time::timeout(std::time::Duration::from_secs(3), lost.cancelled())
        .await
        .unwrap();
}

/// Deliberately opt-in. Runs only probe, permission and lock modes, never start/login.
#[tokio::test]
#[ignore = "requires explicit SHELLFOX_OFFLINE_HELPER and SHELLFOX_OFFLINE_HELPER_SHA256"]
async fn real_retained_go_helper_offline_contract() {
    let executable = real_helper();
    assert_eq!(
        HelperTransport::probe(&executable)
            .await
            .unwrap()
            .protocol_version,
        1
    );
    let scratch = Scratch::new();
    let verifier = PermissionVerifier::new(executable.clone());
    verifier
        .protect(&scratch.0, PrivatePathKind::Directory)
        .await
        .unwrap();
    verifier
        .check(&scratch.0, PrivatePathKind::Directory)
        .await
        .unwrap();
    let file = scratch.0.join("private.json");
    std::fs::write(&file, b"{}").unwrap();
    verifier
        .protect(&file, PrivatePathKind::File)
        .await
        .unwrap();
    verifier.check(&file, PrivatePathKind::File).await.unwrap();
    // std canonicalization adds a verbatim prefix on Windows. The boundary
    // must preserve security while passing the ordinary path required by Go.
    let canonical_directory = std::fs::canonicalize(&scratch.0).unwrap();
    let canonical_file = std::fs::canonicalize(&file).unwrap();
    verifier
        .protect(&canonical_directory, PrivatePathKind::Directory)
        .await
        .unwrap();
    verifier
        .check(&canonical_directory, PrivatePathKind::Directory)
        .await
        .unwrap();
    verifier
        .protect(&canonical_file, PrivatePathKind::File)
        .await
        .unwrap();
    verifier
        .check(&canonical_file, PrivatePathKind::File)
        .await
        .unwrap();
    let missing = scratch.0.join("absent.json");
    assert_eq!(
        verifier.check(&missing, PrivatePathKind::File).await,
        Err(HelperError::PermissionDenied)
    );
    let mut ownership = HostOwnership::acquire(&executable, &canonical_directory)
        .await
        .unwrap()
        .unwrap();
    assert!(ownership.is_alive());
    assert!(HostOwnership::acquire(&executable, &scratch.0)
        .await
        .unwrap()
        .is_none());
    ownership.shutdown_confirmed().await.unwrap();
    ownership.shutdown_confirmed().await.unwrap();
    let mut replacement = HostOwnership::acquire(&executable, &scratch.0)
        .await
        .unwrap()
        .unwrap();
    replacement.shutdown_confirmed().await.unwrap();
}

fn real_helper() -> VerifiedHelper {
    let path = PathBuf::from(
        std::env::var_os("SHELLFOX_OFFLINE_HELPER").expect("explicit helper path required"),
    );
    let hash =
        std::env::var("SHELLFOX_OFFLINE_HELPER_SHA256").expect("explicit helper digest required");
    VerifiedHelper::new(path, hex::decode(hash).unwrap().try_into().unwrap()).unwrap()
}

#[tokio::test]
#[ignore = "requires explicit verified offline Go helper, never starts a node"]
async fn real_go_proves_persistent_state_marker_privacy_without_adopting_credentials() {
    let scratch = Scratch::new();
    let verifier = PermissionVerifier::new(real_helper());
    verifier
        .protect(&scratch.0, PrivatePathKind::Directory)
        .await
        .unwrap();
    let root = IsolatedStateDirectory::create_new_private(scratch.0.join("rust-node"), &verifier)
        .await
        .unwrap();
    let path = root.path().to_owned();
    let marker = path.join(IsolatedStateDirectory::MARKER_FILE);
    let marker_bytes = std::fs::read(&marker).unwrap();
    let credentials = path.join("tailscaled.state");
    std::fs::write(&credentials, b"opaque fixture, not a real node credential").unwrap();
    drop(root);
    let reopened =
        IsolatedStateDirectory::open_existing(std::fs::canonicalize(&path).unwrap(), &verifier)
            .await
            .unwrap();
    assert_eq!(reopened.path(), path);
    drop(reopened);
    assert_eq!(
        std::fs::read(&credentials).unwrap(),
        b"opaque fixture, not a real node credential"
    );
    let linked = scratch.0.join("marker-alias");
    std::fs::hard_link(&marker, &linked).unwrap();
    assert_eq!(
        IsolatedStateDirectory::open_existing(path.clone(), &verifier)
            .await
            .unwrap_err(),
        HelperError::PermissionDenied
    );
    std::fs::remove_file(linked).unwrap();
    #[cfg(windows)]
    {
        // Inherited ACL is deliberately not a protected marker DACL. Reopen
        // must refuse it, not repair an existing file's security descriptor.
        std::fs::remove_file(&marker).unwrap();
        std::fs::write(&marker, &marker_bytes).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = &marker_bytes;
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    assert_eq!(
        verifier.check(&marker, PrivatePathKind::File).await,
        Err(HelperError::PermissionDenied)
    );
    assert_eq!(
        IsolatedStateDirectory::open_existing(path, &verifier)
            .await
            .unwrap_err(),
        HelperError::PermissionDenied
    );
    assert_eq!(
        verifier.check(&marker, PrivatePathKind::File).await,
        Err(HelperError::PermissionDenied)
    );
    assert_eq!(
        std::fs::read(credentials).unwrap(),
        b"opaque fixture, not a real node credential"
    );
}
