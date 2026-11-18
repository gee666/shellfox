//! Read-only security-metadata contract checks. Never reads key bytes, protects
//! ACLs, starts a Host/network, copies a helper, or changes the active Host.
#![cfg(windows)]
use super::*;
use shellfox_ssh::{ProcessRequest, ProcessRunner, SystemProcessRunner};
use std::{path::PathBuf, time::Duration};
fn check(helper: &Path, target: &Path, kind: &str, label: &str) -> Result<bool> {
    let result = SystemProcessRunner.run(&ProcessRequest {
        executable: helper.to_owned(),
        args: vec![
            "--private-permissions".into(),
            target.as_os_str().to_owned(),
            kind.into(),
            "check".into(),
        ],
        timeout: Duration::from_secs(8),
        max_output: 1024,
    })?;
    let value: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    eprintln!(
        "permission-contract {label} success={} stderr_bytes={} response={value}",
        result.success,
        result.stderr.len()
    );
    ensure!(
        result.stderr.is_empty(),
        "private-permissions emitted stderr; not a valid quiet contract"
    );
    if value == serde_json::json!({"ok":true}) {
        ensure!(result.success, "ok response with failed process");
        return Ok(true);
    }
    ensure!(
        !result.success
            && value.as_object().is_some_and(|object| object.len() == 1
                && object
                    .get("error")
                    .is_some_and(serde_json::Value::is_string)),
        "unsupported or malformed private-permissions output"
    );
    Ok(false)
}
#[test]
fn authorized_public_keyscan_reports_its_actual_process_contract() -> Result<()> {
    let program = inputs::program("KEYSCAN")?;
    let started = std::time::Instant::now();
    let result = SystemProcessRunner.run(&ProcessRequest {
        executable: program.path().to_owned(),
        args: [
            "-T",
            "10",
            "-p",
            "22",
            "-t",
            "ed25519,ecdsa,rsa",
            "--",
            "192.168.0.107",
        ]
        .map(std::ffi::OsString::from)
        .to_vec(),
        timeout: Duration::from_secs(15),
        max_output: 256 * 1024,
    });
    match result {
        Ok(output) => {
            eprintln!(
                "authorized-public-keyscan elapsed={}ms success={} stdout={} stderr={}",
                started.elapsed().as_millis(),
                output.success,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            ensure!(
                output.success,
                "actual public keyscan exited unsuccessfully"
            );
        }
        Err(error) => {
            eprintln!(
                "authorized-public-keyscan elapsed={}ms error={error}",
                started.elapsed().as_millis()
            );
            return Err(error.into());
        }
    }
    Ok(())
}
#[test]
fn canonical_helper_checks_own_fixture_before_authorized_key_metadata() -> Result<()> {
    let mut fixture = support::Fixture::new()?;
    ensure!(
        fixture.digest
            == inputs::digest("93a689c3f84d3c36f586876aacbf0f28bfeb10ed7538e014b10dad080b3cffbb")?,
        "canonical93 required"
    );
    let lease = inputs::resource(fixture.helper.clone(), fixture.digest)?;
    let owned = fixture
        .data("unused-leaf")
        .parent()
        .ok_or_else(|| anyhow::anyhow!("fixture parent"))?
        .to_owned();
    ensure!(check(lease.path(),&owned,"directory","own-protected-fixture")?,"canonical helper mode unavailable/denied on its existing protected fixture; no key metadata was touched");
    let original = PathBuf::from("C:/Users/user/.ssh/id_ed25519_omarchy");
    // Same explicit identity, in the two equivalent Windows separator spellings.
    // This only diagnoses argument normalization versus a security check denial.
    let native: PathBuf = original.components().map(|part| part.as_os_str()).collect();
    for (label, path) in [("original", original), ("native", native)] {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("authorized parent"))?;
        let parent_ok = check(
            lease.path(),
            parent,
            "directory",
            &format!("{label}-key-parent"),
        )?;
        let file_ok = check(lease.path(), &path, "file", &format!("{label}-key-file"))?;
        eprintln!(
            "permission-contract {label} metadata_only parent_ok={parent_ok} file_ok={file_ok}"
        );
    }
    drop(lease);
    fixture.success();
    Ok(())
}
