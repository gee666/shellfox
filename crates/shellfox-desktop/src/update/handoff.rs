use super::Platform;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;
#[cfg(unix)]
#[path = "foreground.rs"]
mod foreground;

pub(in crate::update) struct Prepared {
    pub(super) platform: Platform,
    pub(super) work: TempDir,
    pub(super) helper: PathBuf,
    pub(super) file: PathBuf,
    pub(super) sha256: String,
    pub(super) size: u64,
    #[cfg(target_os = "macos")]
    pub(super) stage: Option<TempDir>,
}
impl Prepared {
    /// Only call after confirmed clean shutdown. The script checks the payload
    /// before acknowledging and again after our PID exits.
    pub fn handoff(self, relaunch: bool) -> Result<()> {
        self.verify()?;
        let control = tempfile::Builder::new()
            .prefix("handoff-")
            .tempdir_in(self.work.path())?;
        let mut child = self.start(control.path(), relaunch)?;
        let handoff = (|| {
            wait_for(
                &mut child,
                &control.path().join("ready"),
                Duration::from_secs(60),
            )?;
            fs::File::create_new(control.path().join("commit"))?;
            wait_for(
                &mut child,
                &control.path().join("committed"),
                Duration::from_secs(10),
            )
        })();
        if let Err(error) = handoff {
            let _ = fs::File::create_new(control.path().join("cancel"));
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let _ = control.keep();
        let _ = self.work.keep();
        #[cfg(target_os = "macos")]
        if let Some(stage) = self.stage {
            let _ = stage.keep();
        }
        Ok(())
    }
    fn verify(&self) -> Result<()> {
        let mut hash = Sha256::new();
        let copied = std::io::copy(
            &mut fs::File::open(&self.file)?.take(self.size + 1),
            &mut hash,
        )?;
        ensure!(
            copied == self.size && format!("{:x}", hash.finalize()) == self.sha256,
            "Downloaded installer changed or is missing"
        );
        Ok(())
    }
    /// `shellfox update`, with no app of this installation running. Returns
    /// false when the install continues in the background: Setup.exe kills
    /// every process under the Squirrel root, this one and its CLI included,
    /// so the helper starts it after both exit.
    pub fn install(self) -> Result<bool> {
        #[cfg(windows)]
        {
            self.handoff(false)?;
            Ok(false)
        }
        #[cfg(unix)]
        {
            self.verify()?;
            match &self.platform {
                #[cfg(target_os = "linux")]
                Platform::Deb { .. } => foreground::apt(&self.file)?,
                #[cfg(target_os = "macos")]
                Platform::Mac { bundle } => {
                    let stage = self.stage.as_ref().context("Missing staged app")?;
                    if let Err(error) = foreground::replace_bundle(bundle, stage.path()) {
                        // A failed rollback leaves the previous app in the stage.
                        let _ = self.stage.map(TempDir::keep);
                        return Err(error);
                    }
                }
                Platform::Manual(reason) => anyhow::bail!("{reason}"),
            }
            Ok(true)
        }
    }
    fn start(&self, control: &Path, relaunch: bool) -> Result<Child> {
        let pid = std::process::id();
        let mut command = match &self.platform {
            #[cfg(windows)]
            Platform::Squirrel { updater } => {
                let config = control.join("install.json");
                fs::write(
                    &config,
                    serde_json::to_vec(&serde_json::json!({
                        "pid": pid, "file": self.file, "work": self.work.path(), "updateExe": updater,
                        "sha256": self.sha256, "control": control, "relaunch": relaunch,
                    }))?,
                )?;
                windows_helper(&self.helper, &config)?
            }
            #[cfg(target_os = "linux")]
            Platform::Deb { executable } => {
                let mut command = script_command(&self.helper, pid, self.work.path());
                command
                    .arg(&self.file)
                    .arg(executable)
                    .arg(&self.sha256)
                    .arg(control)
                    .arg(relaunch_arg(relaunch));
                command
            }
            #[cfg(target_os = "macos")]
            Platform::Mac { bundle } => {
                let mut command = script_command(&self.helper, pid, self.work.path());
                command
                    .arg(bundle)
                    .arg(self.stage.as_ref().context("Missing staged app")?.path())
                    .arg(control)
                    .arg(relaunch_arg(relaunch));
                command
            }
            Platform::Manual(reason) => anyhow::bail!("{reason}"),
        };
        Ok(command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?)
    }
}
fn wait_for(child: &mut Child, signal: &Path, timeout: Duration) -> Result<()> {
    let end = Instant::now() + timeout;
    loop {
        ensure!(
            child.try_wait()?.is_none(),
            "Installer helper exited before handoff"
        );
        if signal.is_file() {
            return Ok(());
        }
        ensure!(
            Instant::now() < end,
            "Installer helper did not acknowledge handoff"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}
#[cfg(windows)]
fn windows_helper(helper: &Path, config: &Path) -> Result<Command> {
    use std::os::windows::process::CommandExt;
    let system = std::env::var_os("SystemRoot").context("Windows system root is unavailable")?;
    let mut command =
        Command::new(PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    // A PowerShell 7 PSModulePath inherited from a pwsh terminal hides
    // Windows PowerShell's own modules, such as Get-FileHash.
    command
        .creation_flags(0x0800_0000)
        .env_remove("PSModulePath")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(helper)
        .arg("-Config")
        .arg(config);
    Ok(command)
}
#[cfg(unix)]
fn script_command(helper: &Path, pid: u32, work: &Path) -> Command {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("/bin/sh");
    command
        .process_group(0)
        .arg(helper)
        .arg(pid.to_string())
        .arg(work);
    command
}
#[cfg(unix)]
fn relaunch_arg(relaunch: bool) -> &'static str {
    if relaunch { "1" } else { "0" }
}

#[cfg(all(test, windows))]
#[path = "handoff_tests.rs"]
mod tests;
