//! Launches the real desktop binary with its host process on a fresh data dir
//! and waits until a window has attached. Needs a display and `shellfox-cli`
//! built into the same target directory (`cargo build -p shellfox-cli`).
#[path = "../../test_dev_helper.rs"]
mod dev_helper;

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

/// Kills only the UI this test started if an assertion fails first.
struct Ui(Child);
impl Drop for Ui {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

async fn desktop_registered(data: &Path, helper: &Path, digest: [u8; 32]) -> bool {
    let probe = async {
        let client = LocalClient::connect(data, helper, digest).await.ok()?;
        let reply = client.request(&DesktopLaunchRequest::Discover {}).await;
        let _ = client.disconnect().await;
        Some(matches!(reply, Ok(DesktopLaunchReply::Route(route)) if route.desktop_registered))
    };
    tokio::time::timeout(Duration::from_secs(5), probe)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// Stops the host the desktop started, also when an assertion fails.
struct Host<'a> {
    data: &'a Path,
    helper: &'a Path,
    digest: [u8; 32],
}
impl Drop for Host<'_> {
    fn drop(&mut self) {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async {
            let stop = async {
                let client = LocalClient::connect(self.data, self.helper, self.digest)
                    .await
                    .ok()?;
                client.execute(LocalApiRequest::Stop {}).await.ok()
            };
            let _ = tokio::time::timeout(Duration::from_secs(10), stop).await;
        });
    }
}

#[test]
fn desktop_window_attaches_to_its_own_host_and_quits_with_it() {
    let desktop = PathBuf::from(env!("CARGO_BIN_EXE_shellfox"));
    let cli = desktop.with_file_name(if cfg!(windows) {
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
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("desktop-launch-smoke-")
        .tempdir_in(root)
        .unwrap();
    let data = scratch.path().join("host");
    let mut ui = Ui(Command::new(&desktop)
        .arg("--data-dir")
        .arg(&data)
        .arg("--helper")
        .arg(&helper)
        .args(["--helper-sha256", &hex])
        .stdin(Stdio::null())
        .spawn()
        .unwrap());
    let host = Host {
        data: &data,
        helper: &helper,
        digest,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    while !runtime.block_on(desktop_registered(&data, &helper, digest)) {
        if let Some(status) = ui.0.try_wait().unwrap() {
            panic!("The desktop exited before its window attached: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "No desktop window attached in time"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    // Startup errors that surface after the first window still end the process.
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        ui.0.try_wait().unwrap().is_none(),
        "The desktop exited after attaching"
    );
    drop(host);
    // An orderly stop of its own host quits the desktop quietly.
    let deadline = Instant::now() + Duration::from_secs(30);
    while ui.0.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "The desktop kept running after its host stopped"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}
