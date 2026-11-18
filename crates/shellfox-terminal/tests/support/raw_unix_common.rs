//! Existing real raw-pipe regressions run against the Linux production API.
use super::*;
#[test]
fn raw_binary_stdio_and_real_write_ack_are_preserved() {
    let f = Fixture::new();
    let mut process = f.spawn(&["echo"]);
    let bytes = vec![0, 255, 13, 10, 128];
    process.write(bytes.clone(), true).unwrap().wait().unwrap();
    assert!(process.write(vec![], false).is_err());
    let (stdout, stderr, code) = collect(&mut process);
    assert_eq!(stdout, bytes);
    assert_eq!(stderr, [0, 255, 10]);
    assert_eq!(code, 7);
}
#[test]
fn active_base64_wire_input_limit_is_not_confused_with_output_chunk_size() {
    use shellfox_terminal::raw_process::INPUT_BYTES;
    let f = Fixture::new();
    let mut process = f.spawn(&["echo"]);
    assert!(process.write(vec![1; INPUT_BYTES + 1], false).is_err());
    let bytes: Vec<_> = (0..INPUT_BYTES).map(|n| n as u8).collect();
    process.write(bytes.clone(), true).unwrap().wait().unwrap();
    let (stdout, stderr, code) = collect(&mut process);
    assert_eq!(stdout, bytes);
    assert_eq!(stderr, [0, 255, 10]);
    assert_eq!(code, 7);
}
#[test]
fn prefill_fixture_streams_two_mib_before_bounded_binary_echo() {
    let f = Fixture::new();
    let mut process = f.spawn(&["prefill-echo"]);
    let input: Vec<_> = (0..32768).map(|n| n as u8).collect();
    let receipt = process.write(input.clone(), true).unwrap();
    let (stdout, stderr, code) = collect(&mut process);
    receipt.wait().unwrap();
    let prefix = 2 * 1024 * 1024;
    assert_eq!(stdout.len(), prefix + input.len());
    assert!(stdout[..prefix].iter().all(|b| *b == 123));
    assert_eq!(&stdout[prefix..], input);
    assert_eq!(stderr, [0, 255, 10]);
    assert_eq!(code, 7);
}
#[test]
fn exact_argv_environment_cwd_and_no_tty() {
    let f = Fixture::new();
    let args = [
        "describe",
        "",
        "a b",
        "\"quoted\"",
        "ending\\",
        "a&b|c>%VAR%",
        "emoji 😀",
        "--flag",
    ];
    let mut command = f.command(&args);
    command
        .env
        .insert("FIXTURE_ONLY".into(), "literal secret\nvalue".into());
    let mut process = RawProcess::spawn(&f.helper, command).unwrap();
    process.release_stdin();
    let (stdout, stderr, code) = collect(&mut process);
    assert!(stderr.is_empty());
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["args"], serde_json::json!(&args[1..]));
    assert_eq!(value["cwd"], f.directory.to_str().unwrap());
    assert_eq!(
        value["env"],
        serde_json::json!({"FIXTURE_ONLY":"literal secret\nvalue"})
    );
    for stream in ["stdinTty", "stdoutTty", "stderrTty"] {
        assert_eq!(value[stream], false);
    }
}
#[test]
fn slow_reader_retains_all_output_through_bounded_credit() {
    let f = Fixture::new();
    let mut process = f.spawn(&["flood"]);
    std::thread::sleep(Duration::from_millis(150));
    assert!(process.poll_exit().unwrap().is_none());
    let (stdout, stderr, code) = collect(&mut process);
    assert_eq!(stdout.len(), 3 * 1024 * 1024);
    assert_eq!(stderr.len(), 1024 * 1024);
    assert_eq!(code, 0);
    assert!(stdout.iter().chain(&stderr).all(|b| *b == 123));
}
#[test]
fn release_stdin_is_eof_not_process_termination() {
    let f = Fixture::new();
    let mut process = f.spawn(&["eof"]);
    process.release_stdin();
    process.release_stdin();
    let (stdout, stderr, code) = collect(&mut process);
    assert_eq!(stdout, b"EOF");
    assert!(stderr.is_empty());
    assert_eq!(code, 0);
}
#[test]
fn disconnect_cancels_blocked_input_without_killing_a_live_root() {
    let f = Fixture::new();
    let mut process = f.spawn(&["deferred-eof"]);
    let mut receipts = Vec::new();
    for _ in 0..32 {
        if let Ok(receipt) = process.write(vec![42; 32768], false) {
            receipts.push(receipt);
        } else {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(100));
    let mut pending: Vec<_> = receipts
        .into_iter()
        .filter(|r| r.try_result().unwrap().is_none())
        .collect();
    assert!(!pending.is_empty());
    process.release_stdin();
    let deadline = Instant::now() + Duration::from_millis(500);
    while !pending.is_empty() {
        process.poll_exit().unwrap();
        pending.retain(|receipt| matches!(receipt.try_result(), Ok(None)));
        assert!(
            Instant::now() < deadline,
            "Disconnect left a blocked stdin write owned by a live root"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        process.poll_exit().unwrap().is_none(),
        "Disconnect must not kill the root"
    );
    let (stdout, stderr, code) = collect(&mut process);
    assert_eq!(stdout, b"EOF");
    assert!(stderr.is_empty());
    assert_eq!(code, 0);
}
#[test]
fn blocked_stdin_has_no_early_ack_and_stop_cancels_it() {
    let f = Fixture::new();
    let mut process = f.spawn(&["idle"]);
    let mut receipts = Vec::new();
    let mut rejected = false;
    for _ in 0..32 {
        match process.write(vec![1; 32768], false) {
            Ok(receipt) => receipts.push(receipt),
            Err(_) => {
                rejected = true;
                break;
            }
        }
    }
    assert!(rejected, "Input admission must be bounded");
    std::thread::sleep(Duration::from_millis(100));
    let pending: Vec<_> = receipts
        .into_iter()
        .filter(|receipt| receipt.try_result().unwrap().is_none())
        .collect();
    assert!(
        !pending.is_empty(),
        "Blocked OS writes must not ACK queue admission"
    );
    process.close().unwrap();
    assert!(
        pending.into_iter().any(|receipt| receipt.wait().is_err()),
        "Stop must reject unfinished writes"
    );
}
#[test]
fn natural_root_exit_cleans_descendants_with_inherited_or_redirected_pipes() {
    let f = Fixture::new();
    for mode in ["inherit", "null"] {
        let marker = f.directory.join(format!("orphan-{mode}"));
        let mut process = f.spawn(&["branch", marker.to_str().unwrap(), mode]);
        assert_eq!(collect(&mut process).2, 0);
        std::thread::sleep(Duration::from_millis(1650));
        assert!(!marker.exists(), "Raw descendant survived root exit");
    }
}
#[test]
fn explicit_stop_cleans_owned_tree_but_not_another_owned_job() {
    let f = Fixture::new();
    let killed = f.directory.join("killed");
    let unrelated = f.directory.join("unrelated");
    let mut process = f.spawn(&["idle-branch", killed.to_str().unwrap(), "null"]);
    let mut other = f.spawn(&["delayed", unrelated.to_str().unwrap()]);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(process.try_output(), Some(RawOutput::Data { .. })) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    process.close().unwrap();
    process.close().unwrap();
    assert_eq!(collect(&mut other).2, 0);
    assert!(!killed.exists());
    assert!(unrelated.exists());
}
#[test]
fn assignment_denial_and_missing_go_never_launch_target() {
    let f = Fixture::new();
    let marker = f.directory.join("never-launched");
    let command = f.command(&["delayed", marker.to_str().unwrap()]);
    assert!(RawProcess::spawn_fault(&f.helper, command.clone(), true, false, false).is_err());
    assert!(RawProcess::spawn_fault(&f.helper, command, false, true, false).is_err());
    assert!(!marker.exists());
}
#[test]
fn post_ready_failure_retains_root_and_descendants_until_confirmed_cleanup() {
    let f = Fixture::new();
    let marker = f.directory.join("failed-start-descendant");
    let error = RawProcess::spawn_fault(
        &f.helper,
        f.command(&["idle-branch", marker.to_str().unwrap(), "null"]),
        false,
        false,
        true,
    )
    .unwrap_err();
    let mut retained = error
        .take_process()
        .expect("Post-READY failure must retain root and cgroup");
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if matches!(retained.try_output(), Some(RawOutput::Data { .. })) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    retained.close().unwrap();
    std::thread::sleep(Duration::from_millis(1650));
    assert!(!marker.exists());
}
#[test]
fn unconfirmed_startup_cleanup_returns_original_owner_for_retry() {
    let f = Fixture::new();
    let error =
        RawProcess::spawn_fault(&f.helper, f.command(&["idle"]), true, false, true).unwrap_err();
    let mut retained = error
        .take_process()
        .expect("Unconfirmed cleanup must retain original handles");
    assert!(error.take_process().is_none());
    retained.close().unwrap();
    // No guest was released. Controller cleanup cannot manufacture a guest code.
    assert_eq!(retained.exit_code(), None);
}
