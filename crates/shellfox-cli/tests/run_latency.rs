//! `shellfox run` of a program that prints one line and exits, against a
//! running host. The output and the exit reach the CLI through held reads and
//! raw-process wakes, so a run costs about the process's own startup.
// macOS has no automation processes yet.
#![cfg(windows)]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;
use support::OwnedHost;

use anyhow::{ensure, Context, Result};
use shellfox_service::{local::LocalClient, local_api::LocalApiRequest};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const RUNS: usize = 5;
/// Far above a warm run (process start plus two pipe round trips). A missed
/// wake costs the 25 s hold, so it shows up as a failure, not as noise.
const MAX_MEDIAN: Duration = Duration::from_secs(2);

#[test]
fn a_short_run_returns_promptly() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("No workspace root.")?;
    std::fs::create_dir_all(root.join("tmp"))?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-run-latency-")
        .tempdir_in(root.join("tmp"))?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
    let cli = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_shellfox-cli"));
        command
            .arg("--data-dir")
            .arg(&data)
            .arg("--helper")
            .arg(&helper)
            .arg("--helper-sha256")
            .arg(&hex)
            .args(args);
        command
    };
    let mut host = OwnedHost(
        cli(&["host", "serve"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = (|| {
        runtime.block_on(async {
            let deadline = Instant::now() + Duration::from_secs(60);
            while LocalClient::connect(&data, &helper, digest).await.is_err() {
                ensure!(host.0.try_wait()?.is_none(), "Host exited during startup.");
                ensure!(Instant::now() < deadline, "Host startup deadline.");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            anyhow::Ok(())
        })?;
        let program: &[&str] = if cfg!(windows) {
            &["cmd.exe", "/c", "echo sfrun"]
        } else {
            &["/bin/echo", "sfrun"]
        };
        let mut times = Vec::new();
        for _ in 0..RUNS {
            let started = Instant::now();
            let output = cli(&[&["run", "--"][..], program].concat())
                .stdin(Stdio::null())
                .output()?;
            times.push(started.elapsed());
            ensure!(output.status.success(), "run failed: {output:?}");
            ensure!(
                String::from_utf8_lossy(&output.stdout).contains("sfrun"),
                "missing output: {output:?}"
            );
        }
        times.sort();
        let median = times[RUNS / 2];
        eprintln!("shellfox run median {median:?}, all {times:?}");
        ensure!(
            median <= MAX_MEDIAN,
            "shellfox run median {median:?} exceeds {MAX_MEDIAN:?}"
        );
        Ok(())
    })();
    runtime.block_on(async {
        if let Ok(client) = LocalClient::connect(&data, &helper, digest).await {
            let _ = client.execute(LocalApiRequest::Stop {}).await;
        }
    });
    let _ = host.wait(Duration::from_secs(20));
    result
}
