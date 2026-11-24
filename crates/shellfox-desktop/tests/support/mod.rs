//! The real desktop binary with its host process on a fresh data dir. Needs a
//! display and `shellfox-cli` built into the same target directory
//! (`cargo build -p shellfox-cli`).
#![allow(dead_code)]
#[path = "../../../test_dev_helper.rs"]
mod dev_helper;
pub mod x11;

use shellfox_service::{
    desktop_launch::{DesktopLaunchReply, DesktopLaunchRequest},
    local::LocalClient,
    local_api::LocalApiRequest,
};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Kills only the UI a test started if an assertion fails first.
pub struct Ui(pub Child);
impl Drop for Ui {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub struct Desktop {
    pub binary: PathBuf,
    pub data: PathBuf,
    pub helper: PathBuf,
    pub digest: [u8; 32],
    _scratch: tempfile::TempDir,
}
impl Desktop {
    pub fn new(prefix: &str) -> Self {
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_shellfox"));
        let cli = binary.with_file_name(if cfg!(windows) {
            "shellfox-cli.exe"
        } else {
            "shellfox-cli"
        });
        assert!(
            cli.is_file(),
            "Build shellfox-cli first: {} is missing",
            cli.display()
        );
        let (helper, digest) = dev_helper::helper().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        let scratch = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(root)
            .unwrap();
        Self {
            binary,
            data: scratch.path().join("host"),
            helper,
            digest,
            _scratch: scratch,
        }
    }

    /// The desktop command on this data dir, with `extra` launch arguments.
    pub fn command(&self, extra: &[&str]) -> Command {
        let hex: String = self.digest.iter().map(|b| format!("{b:02x}")).collect();
        let mut command = Command::new(&self.binary);
        command
            .arg("--data-dir")
            .arg(&self.data)
            .arg("--helper")
            .arg(&self.helper)
            .args(["--helper-sha256", &hex])
            .args(extra)
            .stdin(Stdio::null());
        command
    }

    pub fn logs(&self) -> PathBuf {
        shellfox_service::crash_report::logs_dir(&self.data)
    }

    pub fn registered(&self) -> bool {
        let probe = async {
            let client = LocalClient::connect(&self.data, &self.helper, self.digest)
                .await
                .ok()?;
            let reply = client.request(&DesktopLaunchRequest::Discover {}).await;
            let _ = client.disconnect().await;
            Some(matches!(reply, Ok(DesktopLaunchReply::Route(route)) if route.desktop_registered))
        };
        runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(5), probe)
                .await
                .ok()
                .flatten()
                .unwrap_or(false)
        })
    }

    /// Waits until a desktop window attached; fails if `ui` exits first.
    pub fn wait_registered(&self, mut ui: Option<&mut Ui>) {
        let deadline = Instant::now() + Duration::from_secs(90);
        while !self.registered() {
            if let Some(ui) = ui.as_mut()
                && let Some(status) = ui.0.try_wait().unwrap()
            {
                panic!("The desktop exited before its window attached: {status}");
            }
            assert!(
                Instant::now() < deadline,
                "No desktop window attached in time"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// The orderly stop of the host, as `host stop` does.
    pub fn stop_host(&self) {
        runtime().block_on(async {
            let stop = async {
                let client = LocalClient::connect(&self.data, &self.helper, self.digest)
                    .await
                    .ok()?;
                client.execute(LocalApiRequest::Stop {}).await.ok()
            };
            let _ = tokio::time::timeout(Duration::from_secs(10), stop).await;
        });
    }
}

/// Stops the host the desktop started, also when an assertion fails.
pub struct Host<'a>(pub &'a Desktop);
impl Drop for Host<'_> {
    fn drop(&mut self) {
        self.0.stop_host();
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Waits for `ui` to exit, which an orderly host stop causes.
pub fn wait_exit(ui: &mut Ui, within: Duration, what: &str) {
    let deadline = Instant::now() + within;
    while ui.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(200));
    }
}
