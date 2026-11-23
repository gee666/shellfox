use super::*;
use crate::{launch_privilege::new_launch_binding, PtyCommand};
use std::path::PathBuf;
#[test]
#[ignore = "Explicit owned Windows PTY fixture. Set SHELLFOX_PTY_TEST_HELPER and run --ignored."]
fn unbound_and_failed_startups_never_expose_runtime_bound_fact_receipts() {
    let helper = PathBuf::from(
        std::env::var_os("SHELLFOX_PTY_TEST_HELPER").expect("Owned PTY fixture required"),
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .join(format!(
            "pty-facts-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let mut config = PtyConfig::isolated(&root);
    config.supervisor = Some(helper.clone());
    config.command = Some(PtyCommand {
        program: helper.into_os_string(),
        args: vec!["--idle".into()],
    });
    let mut unbound = PtySession::spawn(config.clone()).unwrap();
    assert!(unbound.take_launch_evidence().is_none());
    unbound.shutdown().unwrap();
    let (binding, _) = new_launch_binding();
    let error = PtySession::spawn_windows_inner(
        config,
        SpawnOptions {
            fail_ready_cleanup: true,
            ..SpawnOptions::default()
        },
        Some(binding),
    )
    .unwrap_err();
    let mut retained = error
        .take_session()
        .expect("Unconfirmed failure retains the original controller");
    assert!(
        retained.take_launch_evidence().is_none(),
        "Failed public startup cannot publish a fact capability"
    );
    retained.inner.as_mut().unwrap().inject_cleanup_failure = false;
    retained.shutdown().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
