//! A real host with idle tabs and an attached client keeping every held read
//! outstanding, as the desktop does. At idle the host must answer almost none
//! of them; a read that releases the next read shows up as a high rate.
#![cfg(any(windows, target_os = "macos"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;
use support::OwnedHost;

use anyhow::{ensure, Context, Result};
use shellfox_core::{CreateSession, ProfileId, SessionId};
use shellfox_service::{
    experiment::{ExperimentRequest as Request, ExperimentResponse as Response},
    local::{DesktopReply, DesktopRequest, LocalClient},
    native::{NativePresentation, NativeResponse, NativeViewAttachment},
    power::PowerRequest,
    remote::native::RemoteRequest,
};
use std::{
    cell::Cell,
    future::Future,
    path::Path,
    pin::Pin,
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

const TABS: usize = 3;
/// Shells print their prompts first; only the quiet part counts.
const SETTLE: Duration = Duration::from_secs(4);
const MEASURE: Duration = Duration::from_secs(5);
/// Answers per second the whole client may get at idle.
const MAX_IDLE_RATE: f64 = 5.0;

type Read = Pin<Box<dyn Future<Output = Result<()>>>>;

#[test]
fn an_idle_attached_client_gets_few_answers() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("CLI crate has no workspace root.")?;
    std::fs::create_dir_all(root.join("tmp"))?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-idle-host-")
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
            .arg(hex)
            .args(["host", "serve"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let local = tokio::task::LocalSet::new();
    let result = local.block_on(&runtime, measure(&mut host, &data, &helper, digest));
    runtime.block_on(async {
        if let Ok(client) = LocalClient::connect(&data, &helper, digest).await {
            let _ = client.execute(Request::Stop {}).await;
        }
    });
    let _ = host.wait(Duration::from_secs(20));
    let answers = result?;
    let rate = answers as f64 / MEASURE.as_secs_f64();
    ensure!(
        rate <= MAX_IDLE_RATE,
        "an idle client got {answers} answers in {MEASURE:?} ({rate:.1}/s); a held read is releasing the next one"
    );
    Ok(())
}

async fn connect(host: &mut OwnedHost, data: &Path, helper: &Path, digest: [u8; 32]) -> Result<LocalClient> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let endpoint = data.join("rust-service-v1/endpoint.json");
    loop {
        ensure!(host.0.try_wait()?.is_none(), "CLI host exited before publishing its endpoint.");
        if endpoint.is_file() {
            match LocalClient::connect(data, helper, digest).await {
                Ok(client) => return Ok(client),
                Err(error) if Instant::now() >= deadline => return Err(error.context("Host startup deadline.")),
                Err(_) => {}
            }
        }
        ensure!(Instant::now() < deadline, "Host startup deadline.");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Repeats `read` like a desktop feed: at once after an answer, after a pause
/// after a failure. Counts answers.
fn keep_reading(count: Rc<Cell<usize>>, read: impl Fn() -> Read + 'static) {
    tokio::task::spawn_local(async move {
        loop {
            if read().await.is_err() {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            count.set(count.get() + 1);
        }
    });
}

async fn measure(host: &mut OwnedHost, data: &Path, helper: &Path, digest: [u8; 32]) -> Result<usize> {
    let client = Rc::new(connect(host, data, helper, digest).await?);
    let count = Rc::new(Cell::new(0));
    for index in 0..TABS {
        let title = format!("idle {index}");
        let input = CreateSession {
            request_id: SessionId::new(),
            title: Some(title.clone()),
            cwd: data.to_string_lossy().into_owned(),
            profile_id: ProfileId::new(if cfg!(windows) { "cmd" } else { "login-shell" })?,
        };
        let Response::Snapshot(created) = client.execute(Request::CreateSession { input }).await? else {
            anyhow::bail!("Host did not return the created session.");
        };
        let tab = created
            .sessions
            .iter()
            .find(|session| session.title == title)
            .context("Created session is missing.")?
            .tabs[0]
            .id;
        let DesktopReply::Attached(attachment) =
            client.request(&DesktopRequest::Attach { tab, takeover: false }).await?
        else {
            anyhow::bail!("Host did not attach the tab.");
        };
        let client = client.clone();
        keep_reading(count.clone(), move || {
            let (client, attachment) = (client.clone(), attachment.clone());
            Box::pin(async move {
                let reply = client
                    .request(&DesktopRequest::Presentation {
                        tab,
                        attachment: NativeViewAttachment::Controller(attachment.clone()),
                        action: NativePresentation::Frame { focused: true },
                    })
                    .await?;
                if let NativeResponse::Frame(Some(frame)) = reply.into() {
                    client
                        .request(&DesktopRequest::AcceptFrame { tab, attachment, frame_id: frame.frame_id })
                        .await?;
                }
                Ok(())
            })
        });
    }
    let revision = Rc::new(Cell::new(0));
    {
        let client = client.clone();
        keep_reading(count.clone(), move || {
            let (client, revision) = (client.clone(), revision.clone());
            Box::pin(async move {
                let request = DesktopRequest::ChangedSince { revision: revision.get() };
                if let DesktopReply::Changed(Some(event)) = client.request(&request).await? {
                    revision.set(event.revision);
                }
                Ok(())
            })
        });
    }
    {
        let client = client.clone();
        keep_reading(count.clone(), move || {
            let client = client.clone();
            Box::pin(async move { client.request(&DesktopRequest::ReadTabActivity).await.map(drop) })
        });
    }
    {
        let client = client.clone();
        keep_reading(count.clone(), move || {
            let client = client.clone();
            Box::pin(async move { client.request(&PowerRequest::Status {}).await.map(drop) })
        });
    }
    {
        let client = client.clone();
        keep_reading(count.clone(), move || {
            let client = client.clone();
            Box::pin(async move { client.remote_control(RemoteRequest::State).await.map(drop) })
        });
    }
    tokio::time::sleep(SETTLE).await;
    let before = count.get();
    tokio::time::sleep(MEASURE).await;
    Ok(count.get() - before)
}
