//! While a create or an attach waits for its new shell to start, the host
//! keeps answering other requests: a snapshot sent after it returns first.
// Windows only: a ConPTY start outlasts the 5 ms the test waits before its
// second request; a macOS shell is often up before then.
#![cfg(windows)]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;
use support::OwnedHost;

use anyhow::{ensure, Context, Result};
use shellfox_core::{AddTab, CreateSession, ProfileId, SessionId, TabId};
use shellfox_host::HostRequest;
use shellfox_service::{
    local::{DesktopReply, DesktopRequest, LocalClient},
    local_api::{LocalApiRequest as Request, LocalApiResponse as Response},
};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn a_snapshot_is_answered_while_an_attach_starts_its_shell() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("No workspace root.")?;
    std::fs::create_dir_all(root.join("tmp"))?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-start-overlap-")
        .tempdir_in(root.join("tmp"))?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
    let mut host = OwnedHost(
        Command::new(env!("CARGO_BIN_EXE_shellfox-cli"))
            .arg("--data-dir")
            .arg(&data)
            .arg("--helper")
            .arg(&helper)
            .arg("--helper-sha256")
            .arg(&hex)
            .args(["host", "serve"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(60);
        let client = loop {
            ensure!(host.0.try_wait()?.is_none(), "Host exited during startup.");
            if let Ok(client) = LocalClient::connect(&data, &helper, digest).await {
                break client;
            }
            ensure!(Instant::now() < deadline, "Host startup deadline.");
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let input = CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.to_string_lossy().into_owned(),
            profile_id: ProfileId::new(if cfg!(windows) { "cmd" } else { "login-shell" })?,
        };
        // The create's reply waits for its shell; a snapshot meanwhile doesn't.
        let started = Instant::now();
        let (created, snapshot) = tokio::join!(
            async {
                let reply = client.execute(Request::CreateSession { input }).await;
                (reply, started.elapsed())
            },
            async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                let reply = client.execute(Request::Snapshot {}).await;
                (reply, started.elapsed())
            },
        );
        let Response::Snapshot(created_snapshot) = created.0? else {
            anyhow::bail!("Host did not return the created session.");
        };
        snapshot.0?;
        ensure!(
            snapshot.1 < created.1,
            "the snapshot waited for the create's shell"
        );
        let session = created_snapshot.sessions[0].id;
        // A tab added like the desktop does starts on its first attach.
        let tab = pending_tab(&client, session, &data).await?;
        let started = Instant::now();
        let (attached, snapshot) = tokio::join!(
            async {
                let reply = client
                    .execute(Request::Attach {
                        tab,
                        takeover: false,
                    })
                    .await;
                (reply, started.elapsed())
            },
            async {
                // Sent after the attach reached the host.
                tokio::time::sleep(Duration::from_millis(5)).await;
                let reply = client.execute(Request::Snapshot {}).await;
                (reply, started.elapsed())
            },
        );
        ensure!(
            matches!(attached.0?, Response::Attachment(_)),
            "Host did not attach."
        );
        snapshot.0?;
        eprintln!(
            "attach {:?}, snapshot sent during it {:?}",
            attached.1, snapshot.1
        );
        ensure!(
            snapshot.1 < attached.1,
            "the snapshot waited for the shell start"
        );

        // A detach sent while the attach waits for its start ends that attach:
        // it must not attach a client that already left.
        let input = CreateSession {
            request_id: SessionId::new(),
            title: Some("detached early".into()),
            cwd: data.to_string_lossy().into_owned(),
            profile_id: ProfileId::new(if cfg!(windows) { "cmd" } else { "login-shell" })?,
        };
        let Response::Snapshot(created) = client.execute(Request::CreateSession { input }).await?
        else {
            anyhow::bail!("Host did not return the created session.");
        };
        let session = created
            .sessions
            .iter()
            .find(|s| s.title == "detached early")
            .context("session")?
            .id;
        let tab = pending_tab(&client, session, &data).await?;
        let (attached, detached) = tokio::join!(
            client.execute(Request::Attach {
                tab,
                takeover: false
            }),
            async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                client.execute(Request::Detach {}).await
            }
        );
        detached?;
        ensure!(
            attached.is_err(),
            "an attach finished for a client that had detached"
        );
        client.execute(Request::Stop {}).await?;
        anyhow::Ok(())
    });
    let _ = host.wait(Duration::from_secs(20));
    result
}

/// A tab added through desktop metadata, which starts on its first attach.
async fn pending_tab(client: &LocalClient, session: SessionId, cwd: &Path) -> Result<TabId> {
    let request = DesktopRequest::Metadata(HostRequest::AddTab {
        session_id: session,
        input: AddTab {
            title: None,
            cwd: Some(cwd.to_string_lossy().into_owned()),
            profile_id: ProfileId::new(if cfg!(windows) { "cmd" } else { "login-shell" })?,
        },
    });
    let DesktopReply::Snapshot(snapshot) = client.request(&request).await? else {
        anyhow::bail!("Host did not return the added tab.");
    };
    snapshot
        .sessions
        .iter()
        .find(|s| s.id == session)
        .and_then(|s| s.tabs.last())
        .map(|t| t.id)
        .context("added tab")
}
