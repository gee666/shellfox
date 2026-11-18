#[cfg(windows)]
use crate::automation::test_owned as owned;
#[cfg(windows)]
use anyhow::{ensure, Context, Result};
#[cfg(windows)]
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
#[test]
fn privileged_discriminators_precede_normal_startup_and_logging() {
    let source = include_str!("main.rs");
    let dispatch = source
        .find("shellfox_service::ssh::dispatch_early_entry(&arguments)")
        .unwrap();
    let normal = source.find("match run()").unwrap();
    assert!(dispatch < normal);
    assert!(source.contains("i32::from_ne_bytes(code.to_ne_bytes())"));
    assert!(
        !source.contains("shellfox_terminal::supervisor_entry()"),
        "mode policy belongs only to shared dispatcher"
    );
}
#[test]
#[cfg(windows)]
#[ignore = "explicit rebuilt CLI fixture; fake broker/token only, no network/password"]
fn configured_askpass_failure_is_silent_and_does_not_enter_normal_cli() -> Result<()> {
    let cli = PathBuf::from(
        std::env::var_os("SHELLFOX_CLI_TEST_EXECUTABLE")
            .context("explicit compiled CLI fixture")?,
    );
    ensure!(
        cli.is_absolute() && cli.is_file(),
        "Absolute owned CLI fixture required."
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root)?;
    let scratch = tempfile::Builder::new()
        .prefix("askpass-dispatch-")
        .tempdir_in(root)?;
    let endpoint = format!(
        "\\\\.\\pipe\\shellfox-rust-askpass-missing-{}",
        uuid::Uuid::new_v4()
    );
    let mut child = owned::OwnedHost(
        Command::new(cli)
            .env_clear()
            .env(
                "SystemRoot",
                std::env::var_os("SystemRoot").context("Windows system root")?,
            )
            .env("SHELLFOX_SSH_ASKPASS_ENDPOINT", endpoint)
            .env("SHELLFOX_SSH_ASKPASS_DIRECTORY", scratch.path())
            .env("SHELLFOX_SSH_ASKPASS_TOKEN", "owned-fake-token-no-password")
            .arg("owned fake prompt")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = child.0.stdout.take().context("stdout")?;
    let stderr = child.0.stderr.take().context("stderr")?;
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(8193).read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(8193).read_to_end(&mut bytes).map(|_| bytes)
    });
    let status = child.wait(Duration::from_secs(5))?;
    ensure!(
        status.code() == Some(1),
        "Missing owned broker did not fail closed."
    );
    ensure!(
        out.join()
            .map_err(|_| anyhow::anyhow!("reader"))??
            .is_empty(),
        "Askpass failure emitted captured answer/output."
    );
    ensure!(
        err.join()
            .map_err(|_| anyhow::anyhow!("reader"))??
            .is_empty(),
        "Askpass failure entered normal CLI logging or dumped diagnostics."
    );
    Ok(())
}
