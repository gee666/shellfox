use super::{serve_with_boot_status, LocalBootStatus};
use std::{path::Path, sync::{Arc, Mutex}};

#[tokio::test(flavor = "current_thread")]
async fn pure_preflight_and_hash_rejects_report_clean_without_helpers_or_actor() {
    let data = crate::tests::scratch();
    for (directory, helper) in [
        (Path::new("relative-data").to_owned(), std::env::current_exe().unwrap()),
        (data.path().to_owned(), data.path().join("missing-helper")),
        // A failed read-only artifact check is still before any helper/owner.
        (data.path().to_owned(), std::env::current_exe().unwrap()),
    ] {
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let observed = statuses.clone();
        let result = serve_with_boot_status(&directory, &helper, [0;32], (),
            |_| panic!("Failed preflight must not publish readiness."),
            move |status| observed.lock().unwrap().push(status)).await;
        assert!(result.is_err());
        assert_eq!(*statuses.lock().unwrap(), vec![LocalBootStatus::NoOwnerClean]);
        assert!(!data.path().join(shellfox_host::STORE_FILENAME).exists());
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires a trusted isolated helper fixture that never responds; no live app helpers"]
async fn native_utility_timeout_retains_status_and_never_claims_no_owner() {
    let data = crate::tests::scratch();
    let helper = std::path::PathBuf::from(std::env::var_os("SHELLFOX_BOOT_TIMEOUT_HELPER").expect("fixture path"));
    let hash = std::env::var("SHELLFOX_BOOT_TIMEOUT_HELPER_SHA256").expect("fixture digest");
    assert!(hash.len() == 64 && hash.bytes().all(|byte|byte.is_ascii_hexdigit()));
    let mut digest = [0;32];
    for (index, byte) in digest.iter_mut().enumerate() { *byte = u8::from_str_radix(&hash[index*2..index*2+2],16).unwrap(); }
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let observed = statuses.clone();
    let result = serve_with_boot_status(data.path(), &helper, digest, (),
        |_| panic!("Timed-out helper must not publish readiness."),
        move |status| observed.lock().unwrap().push(status)).await;
    assert!(result.is_err());
    assert_eq!(*statuses.lock().unwrap(), vec![LocalBootStatus::RetainedCleanup]);
    assert!(!data.path().join(shellfox_host::STORE_FILENAME).exists());
    assert!(!data.path().join("rust-service-v1/endpoint.json").exists());
}
