//! Desktop update worker. The GUI never accepts asset URLs or installer commands.
mod feed;
mod platform;
pub(crate) mod cli;

pub(crate) mod view;
use anyhow::Result;
use feed::{Feed, Release};
use platform::{Platform, Prepared};
use semver::Version;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;

pub(crate) use feed::RELEASES;
const CHECK_INTERVAL: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DownloadFailure {
    ReleaseCheck,
    DownloadOrVerification,
}
impl DownloadFailure {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::ReleaseCheck => {
                "Could not check the latest release. Try the download again when connected."
            }
            Self::DownloadOrVerification => {
                "The update could not be downloaded or verified. Try again, or download it from the releases page."
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Notice {
    Available {
        version: Version,
        reason: Option<&'static str>,
        error: Option<DownloadFailure>,
    },
    Downloading {
        version: Version,
        received: u64,
        total: u64,
    },
    Ready,
}
#[derive(Default)]
struct State {
    preference: Option<bool>,
    manual_download: bool,
    manual_close: bool,
    phase: Option<Notice>,
    prepared: Option<Prepared>,
    final_close: Option<bool>,
    serial: u64,
}
impl State {
    /// Returns the new serial, or None when only download progress moved within
    /// the same percent, which the notice doesn't show.
    fn publish(&mut self, notice: Option<Notice>) -> Option<u64> {
        let visible = !matches!(
            (&self.phase, &notice),
            (
                Some(Notice::Downloading { version: old, received: before, total }),
                Some(Notice::Downloading { version, received, total: size }),
            ) if old == version && total == size
                && percent(*before, *total) == percent(*received, *size)
        );
        self.phase = notice;
        visible.then(|| {
            self.serial = self.serial.wrapping_add(1);
            self.serial
        })
    }
}
fn percent(received: u64, total: u64) -> u64 {
    (received.min(total) * 100).checked_div(total).unwrap_or(0)
}
#[derive(Default)]
struct Worker {
    state: Mutex<State>,
    wake: Notify,
    /// The notice serial; the UI awaits changes instead of polling.
    changes: tokio::sync::watch::Sender<u64>,
}
impl Worker {
    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }
    fn publish(&self, notice: Option<Notice>, update: impl FnOnce(&mut State)) {
        if let Some(serial) = self.with(|state| {
            update(state);
            state.publish(notice)
        }) {
            self.changes.send_replace(serial);
        }
    }
}

pub(crate) struct RestartRequest(Arc<Worker>);
impl Drop for RestartRequest {
    fn drop(&mut self) {
        self.0.with(|state| state.manual_close = false);
    }
}

pub(crate) struct Updater {
    worker: Arc<Worker>,
    runtime: Option<tokio::runtime::Runtime>,
}
impl Updater {
    pub fn start() -> Result<Self> {
        let current = env!("SHELLFOX_VERSION").parse()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let worker = Arc::new(Worker::default());
        runtime.spawn(run(worker.clone(), current));
        Ok(Self {
            worker,
            runtime: Some(runtime),
        })
    }
    pub fn preference(&self, enabled: bool) {
        let changed = self
            .worker
            .with(|s| s.preference.replace(enabled) != Some(enabled));
        if changed {
            self.worker.wake.notify_one();
        }
    }
    pub fn request_download(&self) {
        self.worker.with(|s| {
            if matches!(&s.phase, Some(Notice::Available { reason: None, .. })) {
                s.manual_download = true;
            }
        });
        self.worker.wake.notify_one();
    }
    pub fn notice(&self) -> Option<Notice> {
        self.worker.with(|s| s.phase.clone())
    }
    pub fn notice_snapshot(&self) -> (u64, Option<Notice>) {
        self.worker.with(|s| (s.serial, s.phase.clone()))
    }
    pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.worker.changes.subscribe()
    }
    pub fn install_pending(&self) -> bool {
        self.worker
            .with(|state| state.manual_close || state.final_close.is_some())
    }
    pub fn request_restart(&self) -> Option<RestartRequest> {
        let accepted = self.worker.with(|state| {
            if state.manual_close || !matches!(state.phase, Some(Notice::Ready)) {
                return false;
            }
            state.manual_close = true;
            true
        });
        accepted.then(|| RestartRequest(self.worker.clone()))
    }
    pub fn confirm_close(&self, restart: bool) {
        self.worker.with(|s| {
            if s.prepared.is_some() && (restart || s.preference == Some(true)) {
                s.final_close = Some(restart);
            }
        });
    }
    pub fn install_on_exit(&self) -> Result<()> {
        let prepared = self
            .worker
            .with(|s| s.final_close.take().zip(s.prepared.take()));
        if let Some((restart, staged)) = prepared {
            staged.handoff(restart)?;
        }
        Ok(())
    }
}
impl Drop for Updater {
    fn drop(&mut self) {
        // A slow or blocked download must not prevent a normal quit.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

async fn run(worker: Arc<Worker>, current: Version) {
    let Ok(feed) = Feed::new() else {
        return;
    };
    let Ok(platform) = tokio::task::spawn_blocking(Platform::detect).await else {
        return;
    };
    let mut release = None;
    let mut attempted = None;
    let mut failure = None;
    let mut next_check = Instant::now();
    loop {
        let manual = worker.with(|s| std::mem::take(&mut s.manual_download));
        let automatic = worker.with(|s| s.preference == Some(true));
        let auto_requested = automatic
            && can_download(&release, &platform, &current, |r| {
                attempted.as_ref() != Some(&r.version)
            });
        let periodic = Instant::now() >= next_check;
        if manual || auto_requested || periodic {
            next_check = Instant::now() + CHECK_INTERVAL;
            match feed.check(|version| platform.asset_name(version)).await {
                Ok(fresh) => {
                    if release
                        .as_ref()
                        .is_none_or(|old: &Release| old.version != fresh.version)
                    {
                        failure = None;
                    }
                    release = Some(fresh);
                    publish(&worker, &release, &platform, &current, failure);
                    let automatic = worker.with(|s| s.preference == Some(true));
                    let download = can_download(&release, &platform, &current, |r| {
                        manual || (automatic && attempted.as_ref() != Some(&r.version))
                    });
                    if download
                        && let Some((r, asset)) = release
                            .as_ref()
                            .and_then(|r| r.asset.as_ref().map(|a| (r, a)))
                    {
                        attempted = Some(r.version.clone());
                        worker.publish(
                            Some(Notice::Downloading {
                                version: r.version.clone(),
                                received: 0,
                                total: asset.size,
                            }),
                            |_| {},
                        );
                        match stage(&feed, &platform, r, asset, &worker).await {
                            Ok(staged) => {
                                worker.publish(Some(Notice::Ready), |s| s.prepared = Some(staged));
                                return;
                            }
                            Err(error) => {
                                eprintln!(
                                    "Shellfox update could not be downloaded or verified: {error:#}"
                                );
                                failure = Some(DownloadFailure::DownloadOrVerification);
                                publish(&worker, &release, &platform, &current, failure);
                            }
                        }
                    }
                }
                Err(_) if manual || auto_requested => {
                    if auto_requested {
                        attempted = release.as_ref().map(|r| r.version.clone());
                    }
                    if manual {
                        failure = Some(DownloadFailure::ReleaseCheck);
                        publish(&worker, &release, &platform, &current, failure);
                    }
                }
                Err(_) => {}
            }
        }
        tokio::select! {
            () = worker.wake.notified() => {},
            () = tokio::time::sleep_until(next_check.into()) => {},
        }
    }
}
fn can_download(
    release: &Option<Release>,
    platform: &Platform,
    current: &Version,
    requested: impl FnOnce(&Release) -> bool,
) -> bool {
    platform.reason().is_none()
        && release
            .as_ref()
            .is_some_and(|r| r.version > *current && r.asset.is_some() && requested(r))
}
fn publish(
    worker: &Worker,
    release: &Option<Release>,
    platform: &Platform,
    current: &Version,
    error: Option<DownloadFailure>,
) {
    let notice = release
        .as_ref()
        .filter(|r| r.version > *current)
        .map(|r| Notice::Available {
            version: r.version.clone(),
            reason: platform.reason().or_else(|| {
                r.asset
                    .is_none()
                    .then_some("This release has no verified installer for this platform. Download it from the releases page.")
            }),
            error,
        });
    worker.publish(notice, |_| {});
}
async fn stage(
    feed: &Feed,
    platform: &Platform,
    release: &Release,
    asset: &feed::Asset,
    worker: &Worker,
) -> Result<Prepared> {
    let work = tempfile::Builder::new()
        .prefix("shellfox-update-")
        .tempdir()?;
    let file = work.path().join(&asset.name);
    feed.download(asset, &file, |received| {
        worker.publish(
            Some(Notice::Downloading {
                version: release.version.clone(),
                received,
                total: asset.size,
            }),
            |_| {},
        );
    })
    .await?;
    let (platform, version, asset) = (platform.clone(), release.version.clone(), asset.clone());
    tokio::task::spawn_blocking(move || platform.prepare(file, &version, work, &asset)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_window_can_request_restart_until_cancel_or_completion() {
        let updater = Updater {
            worker: Arc::new(Worker::default()),
            runtime: None,
        };
        assert!(updater.request_restart().is_none());
        updater
            .worker
            .publish(Some(Notice::Ready), |_| {});
        let first = updater.request_restart().expect("first confirmation");
        assert!(updater.request_restart().is_none());
        drop(first);
        assert!(updater.request_restart().is_some());
    }
}
