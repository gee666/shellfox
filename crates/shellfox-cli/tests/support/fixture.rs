//! A real host on isolated data, driven through the real `shellfox` CLI.
use crate::support::OwnedHost;
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

pub struct Fixture {
    pub scratch: tempfile::TempDir,
    pub data: PathBuf,
    pub helper: PathBuf,
    pub digest: [u8; 32],
    exe: PathBuf,
    host: Option<OwnedHost>,
}

impl Fixture {
    #[allow(dead_code, reason = "package-layout tests start their own copy")]
    pub fn start(helper: PathBuf, digest: [u8; 32]) -> Result<Self> {
        Self::start_as(env!("CARGO_BIN_EXE_shellfox-cli").into(), helper, digest)
    }
    /// A host run by `exe`, a copy of this CLI placed in some package layout.
    pub fn start_as(exe: PathBuf, helper: PathBuf, digest: [u8; 32]) -> Result<Self> {
        let tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .context("workspace")?
            .join("tmp");
        std::fs::create_dir_all(&tmp)?;
        let scratch = tempfile::Builder::new()
            .prefix("cli-manager-")
            .tempdir_in(&tmp)?;
        let data = scratch.path().join("data");
        std::fs::create_dir(&data)?;
        let mut fixture = Self {
            scratch,
            data,
            helper,
            digest,
            exe,
            host: None,
        };
        let host = fixture
            .command(&["host", "run", "--json"])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        fixture.host = Some(OwnedHost(host));
        let deadline = Instant::now() + Duration::from_secs(60);
        while fixture.json(&["host", "status", "--json"]).is_err() {
            let exited = fixture
                .host
                .as_mut()
                .map(|h| h.0.try_wait())
                .transpose()?
                .flatten();
            ensure!(
                Instant::now() < deadline && exited.is_none(),
                "Host did not start."
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(fixture)
    }
    /// `shellfox-cli --data-dir ... WORDS`, the command line a terminal would also run.
    pub fn argv(&self, words: &[&str]) -> Vec<String> {
        let hash: String = self.digest.iter().map(|b| format!("{b:02x}")).collect();
        let mut argv = vec![
            "--data-dir".to_owned(),
            self.data.display().to_string(),
            "--helper".into(),
            self.helper.display().to_string(),
            "--helper-sha256".into(),
            hash,
        ];
        argv.extend(words.iter().map(|w| (*w).to_owned()));
        argv
    }
    pub fn command(&self, words: &[&str]) -> Command {
        let mut command = Command::new(&self.exe);
        command
            .args(self.argv(words))
            .current_dir(self.scratch.path())
            .env_remove("SHELLFOX_TERMINAL_MARKER")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null());
        command
    }
    pub fn output(&self, words: &[&str]) -> Result<Output> {
        Ok(self.command(words).output()?)
    }
    /// `--json` success value.
    pub fn json(&self, words: &[&str]) -> Result<Value> {
        let (code, reply) = self.reply(words)?;
        ensure!(
            code == 0 && reply["ok"] == true,
            "{words:?} failed: {reply}"
        );
        Ok(reply["value"].clone())
    }
    /// Exit code and the `--json` record, success or failure.
    pub fn reply(&self, words: &[&str]) -> Result<(i32, Value)> {
        let output = self.output(words)?;
        let reply: Value = serde_json::from_slice(&output.stdout).with_context(|| {
            format!(
                "{words:?}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
        Ok((output.status.code().unwrap_or(-1), reply))
    }
    /// Plain-mode stdout of a successful command.
    #[allow(dead_code, reason = "not every test reads plain output")]
    pub fn plain(&self, words: &[&str]) -> Result<String> {
        let output = self.output(words)?;
        ensure!(
            output.status.success(),
            "{words:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }
    pub fn stop(mut self) -> Result<()> {
        let stopped = self.output(&["host", "stop", "--yes", "--json"])?;
        let status = self
            .host
            .as_mut()
            .context("host")?
            .wait(Duration::from_secs(30))?;
        ensure!(
            stopped.status.success() && status.success(),
            "Host did not stop cleanly."
        );
        Ok(())
    }
}
