use super::*;
use crate::launch_privilege::new_launch_binding;
use std::{collections::BTreeMap, path::PathBuf};
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn retained_initial_snapshot_cannot_extend_original_job_kill_on_close_lifetime() {
    let helper = PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER").expect("Owned raw fixture required"),
    );
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tmp")
        .join(format!(
            "raw-facts-custody-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let marker = directory.join("descendant-survived");
    let command = RawCommand {
        executable: helper.to_str().unwrap().into(),
        args: vec![
            "idle-branch".into(),
            marker.to_str().unwrap().into(),
            "null".into(),
        ],
        cwd: directory.to_str().unwrap().into(),
        env: BTreeMap::new(),
    };
    let (binding, verifier) = new_launch_binding();
    let mut process = RawProcess::spawn_with_launch_binding(&helper, command, binding).unwrap();
    let evidence = process.take_launch_evidence().unwrap();
    let facts = evidence.facts();
    assert!(verifier.matches(&evidence));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(process.try_output(), Some(RawOutput::Data { .. })) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    // Simulate abrupt controller Job-handle loss, NOT normal explicit terminate.
    // Keep only the ORIGINAL process handle to observe death. No PID lookup and
    // no Job duplicate may be hidden in the published scalar snapshot/verifier.
    let Inner {
        mut child,
        job,
        mut input,
        output,
        readers,
        ..
    } = process.inner.take().unwrap();
    drop(job);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut killed_while_snapshot_retained = false;
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            killed_while_snapshot_retained = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(evidence.facts(), facts);
    assert!(verifier.matches(&evidence));
    // Release snapshots before fallback cleanup if a future regression retains
    // an unwanted Job handle. Existing owned handles remain available throughout.
    drop(evidence);
    drop(verifier);
    output.discard();
    if let Some(input) = &input {
        input.close();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "Original process failed to exit after Job custody release"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    if let Some(input) = &mut input {
        while !input.join().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    for reader in readers {
        while !reader.is_finished() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        reader.join().unwrap();
    }
    drop(child);
    drop(process);
    std::thread::sleep(Duration::from_millis(1650));
    let descendant_survived = marker.exists();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(
        killed_while_snapshot_retained,
        "Facts receipt extended KILL_ON_JOB_CLOSE lifetime"
    );
    assert!(!descendant_survived);
}
