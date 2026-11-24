//! Several tabs redraw full screens at 30 fps while a client reads and acknowledges
//! all of them, like a desktop with agents running in each tab. The host must keep
//! answering, admit request bursts, and hold a stable fd count.
//!
//! Runs only with SHELLFOX_HOST_SOAK=1 (Linux CI); SHELLFOX_HOST_SOAK_SECS sets the length.
#![cfg(target_os = "linux")]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{bail, ensure, Context, Result};
use fixture::Fixture;
use shellfox_core::{CreateSession, ProfileId, SessionId, TabId};
use shellfox_service::{
    local::LocalClient,
    local_api::{LocalApiRequest as Request, LocalApiResponse as Response},
    Attachment,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const TABS: usize = 8;
/// Concurrent requests per burst: more than the owner inbox holds.
const BURST: usize = 48;
/// fds a tab may hold beyond the warm-up count when the soak ends.
const FD_SLACK: usize = 16;
const CHECK_EVERY: Duration = Duration::from_secs(5);
/// A redraw loop with a child process tree, like a TUI agent.
const REDRAW: &str = "sleep 600 & (sleep 600 &); while :; do printf '\\033[H'; i=0; \
    while [ $i -lt 40 ]; do printf 'row %s %s\\033[K\\n' $i $RANDOM$RANDOM$RANDOM; \
    i=$((i+1)); done; sleep 0.03; done";

#[test]
fn many_busy_tabs_keep_the_host_answering() -> Result<()> {
    if std::env::var_os("SHELLFOX_HOST_SOAK").is_none_or(|value| value != "1") {
        eprintln!("Skipped: set SHELLFOX_HOST_SOAK=1");
        return Ok(());
    }
    let length = Duration::from_secs(
        std::env::var("SHELLFOX_HOST_SOAK_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(180),
    );
    let (helper, digest) = dev_helper::helper()?;
    let fixture = Fixture::start(helper, digest)?;
    let pid = fixture.host_pid().context("host pid")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(soak(&fixture, pid, length));
    let stopped = fixture.stop();
    result?;
    stopped
}

async fn soak(fixture: &Fixture, pid: u32, length: Duration) -> Result<()> {
    let client =
        Arc::new(LocalClient::connect(&fixture.data, &fixture.helper, fixture.digest).await?);
    let stop = Arc::new(AtomicBool::new(false));
    let mut readers = Vec::new();
    let mut tabs = Vec::new();
    for _ in 0..TABS {
        let tab = create(&client, fixture.scratch.path()).await?;
        let attachment = attach(&client, tab).await?;
        let frames = Arc::new(AtomicU64::new(0));
        tabs.push((tab, attachment.clone(), frames.clone()));
        let (client, stop) = (client.clone(), stop.clone());
        // Each tab is read like a view: poll, acknowledge, repeat.
        readers.push(tokio::spawn(async move {
            let mut sequence = recover(&client, tab, &attachment).await?;
            acknowledge(&client, tab, &attachment, sequence).await?;
            input(&client, tab, &attachment, &format!("{REDRAW}\r")).await?;
            while !stop.load(Ordering::Relaxed) {
                read(&client, tab, &attachment, &mut sequence).await?;
                frames.fetch_add(1, Ordering::Relaxed);
            }
            anyhow::Ok(())
        }));
    }
    let started = Instant::now();
    let mut baseline = None;
    let mut check = 0_usize;
    while started.elapsed() < length {
        tokio::time::sleep(CHECK_EVERY).await;
        check += 1;
        let burst = (0..BURST).map(|_| client.execute(Request::Snapshot {}));
        for reply in futures_util::future::join_all(burst).await {
            reply.context("A request in a burst failed")?;
        }
        let (tab, attachment, _) = &tabs[check % TABS];
        let resize = Request::Resize {
            tab: *tab,
            attachment: attachment.clone(),
            cols: 80 + (check % 40) as u16,
            rows: 24 + (check % 10) as u16,
        };
        client.execute(resize).await?;
        let answered = Instant::now();
        client.execute(Request::Snapshot {}).await?;
        ensure!(
            answered.elapsed() < Duration::from_secs(5),
            "The host took {:?} to answer a snapshot.",
            answered.elapsed()
        );
        let fds = std::fs::read_dir(format!("/proc/{pid}/fd"))?.count();
        if started.elapsed() > Duration::from_secs(20) {
            let warm = *baseline.get_or_insert(fds);
            ensure!(
                fds <= warm + FD_SLACK,
                "Host fds grew from {warm} to {fds}."
            );
        }
        let frames: Vec<u64> = tabs
            .iter()
            .map(|(_, _, frames)| frames.load(Ordering::Relaxed))
            .collect();
        eprintln!("soak {:?}: fds={fds} reads={frames:?}", started.elapsed());
        ensure!(
            readers.iter().all(|reader| !reader.is_finished()),
            "A tab reader stopped."
        );
    }
    ensure!(baseline.is_some(), "The soak ended before its fd baseline.");
    stop.store(true, Ordering::Relaxed);
    for (reader, (_, _, frames)) in readers.into_iter().zip(&tabs) {
        reader.await??;
        ensure!(
            frames.load(Ordering::Relaxed) > 100,
            "A tab showed almost no output."
        );
    }
    Ok(())
}

/// One frame-sized read: polls the tab's output and acknowledges it.
async fn read(
    client: &LocalClient,
    tab: TabId,
    attachment: &Attachment,
    sequence: &mut u64,
) -> Result<()> {
    let poll = Request::Poll {
        tab,
        attachment: attachment.clone(),
    };
    let last = match client.execute(poll).await? {
        Response::Output(chunks) => chunks.iter().map(|chunk| chunk.sequence).max(),
        Response::RecoveryRequired {} => Some(recover(client, tab, attachment).await?),
        Response::TerminalExited { .. } => bail!("A soak shell exited."),
        _ => bail!("Invalid poll response."),
    };
    if let Some(last) = last.filter(|last| last > sequence) {
        acknowledge(client, tab, attachment, last).await?;
        *sequence = last;
    }
    Ok(())
}

async fn acknowledge(
    client: &LocalClient,
    tab: TabId,
    attachment: &Attachment,
    sequence: u64,
) -> Result<()> {
    let ack = Request::Acknowledge {
        tab,
        attachment: attachment.clone(),
        sequence,
    };
    client.execute(ack).await?;
    Ok(())
}

/// The tab's whole screen and the sequence it covers, as a view starts.
async fn recover(client: &LocalClient, tab: TabId, attachment: &Attachment) -> Result<u64> {
    let recover = Request::Recover {
        tab,
        attachment: attachment.clone(),
    };
    match client.execute(recover).await? {
        Response::Recovery { last_sequence, .. } => Ok(last_sequence),
        _ => bail!("Invalid recovery response."),
    }
}

async fn create(client: &LocalClient, cwd: &std::path::Path) -> Result<TabId> {
    let input = CreateSession {
        request_id: SessionId::new(),
        title: Some("host soak".into()),
        cwd: cwd.to_str().context("UTF-8 path")?.into(),
        profile_id: ProfileId::new("login-shell")?,
    };
    match client.execute(Request::CreateSession { input }).await? {
        Response::Snapshot(snapshot) => snapshot.selection.tab_id.context("no selected tab"),
        _ => bail!("Invalid create response."),
    }
}

async fn attach(client: &LocalClient, tab: TabId) -> Result<Attachment> {
    match client
        .execute(Request::Attach {
            tab,
            takeover: false,
        })
        .await?
    {
        Response::Attachment(attachment) => Ok(attachment),
        _ => bail!("Invalid attachment response."),
    }
}

async fn input(
    client: &LocalClient,
    tab: TabId,
    attachment: &Attachment,
    text: &str,
) -> Result<()> {
    let input = Request::Input {
        tab,
        attachment: attachment.clone(),
        bytes: text.as_bytes().to_vec(),
    };
    ensure!(
        matches!(client.execute(input).await?, Response::Done {}),
        "Invalid input response."
    );
    Ok(())
}
