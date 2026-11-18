use crate::input::InputWriter;
use crate::reader::OutputReader;
use crate::ExitStatus;
use anyhow::{ensure, Context, Result};
use portable_pty::{Child, MasterPty};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct Session {
    pub(crate) child: Box<dyn Child + Send + Sync>,
    pub(crate) inspection: Option<crate::inspection::InspectionCapability>,
    pub(crate) program_env: Option<crate::TrustedProgramEnv>,
    pub(crate) launch_evidence: Option<crate::NativeLaunchEvidence>,
    pub(crate) master: Option<Box<dyn MasterPty + Send>>,
    pub(crate) writer: InputWriter,
    pub(crate) output: OutputReader,
    pub(crate) closer: ConsoleCloser,
    pub(crate) status: Option<ExitStatus>,
    pub(crate) ready: bool,
    pub(crate) released: bool,
    pub(crate) cleanup_started: bool,
    #[cfg(feature = "test-support")]
    pub(crate) inject_cleanup_failure: bool,
    #[cfg(windows)]
    pub(crate) job: Option<crate::windows::Job>,
}

impl Session {
    pub(crate) fn poll_root(&mut self) -> Result<Option<ExitStatus>> {
        if self.status.is_none() {
            #[cfg(windows)]
            {
                self.status = crate::windows::poll_child(self.child.as_ref())?;
            }
            #[cfg(not(windows))]
            {
                self.status = self.child.try_wait()?;
            }
        }
        Ok(self.status.clone())
    }

    pub(crate) fn begin_cleanup(&mut self) -> Result<()> {
        self.ready = false;
        #[cfg(feature = "test-support")]
        ensure!(
            !self.inject_cleanup_failure,
            "injected cleanup failure; owners retained"
        );
        #[cfg(windows)]
        {
            self.job
                .as_ref()
                .context("owned Job missing")?
                .terminate()?;
            if !self.released {
                crate::windows::terminate_unreleased(self.child.as_ref())?;
            }
        }
        self.cleanup_started = true;
        Ok(())
    }

    pub(crate) fn processes_gone(&mut self) -> Result<bool> {
        let root_gone = self.poll_root()?.is_some();
        #[cfg(windows)]
        {
            Ok(root_gone
                && self
                    .job
                    .as_ref()
                    .context("owned Job missing")?
                    .active_processes()?
                    == 0)
        }
        #[cfg(not(windows))]
        {
            Ok(root_gone)
        }
    }

    pub(crate) fn close_console(&mut self) -> Result<()> {
        self.writer.stop();
        if let Some(master) = self.master.take() {
            if let Err(mpsc::SendError(retained)) = self.closer.tx.send(master) {
                self.master = Some(retained);
                anyhow::bail!("PTY close worker unavailable; console retained");
            }
        }
        Ok(())
    }

    pub(crate) fn shutdown(&mut self) -> Result<()> {
        self.output.cancel();
        self.begin_cleanup()?;
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        while !self.processes_gone()? {
            ensure!(
                Instant::now() < deadline,
                "owned process cleanup timed out; owners retained"
            );
            thread::sleep(Duration::from_millis(2));
        }
        self.close_console()?;
        while !self.output.finished()
            || !self.writer.finished()
            || !self
                .closer
                .thread
                .as_ref()
                .is_none_or(JoinHandle::is_finished)
        {
            self.writer.stop();
            ensure!(
                Instant::now() < deadline,
                "PTY pipe cleanup timed out; owners retained"
            );
            thread::sleep(Duration::from_millis(2));
        }
        self.output.join()?;
        self.writer.join()?;
        if let Some(thread) = self.closer.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("PTY close worker panicked"))?;
        }
        #[cfg(windows)]
        self.job.take();
        Ok(())
    }
}

pub(crate) struct ConsoleCloser {
    pub(crate) tx: SyncSender<Box<dyn MasterPty + Send>>,
    pub(crate) thread: Option<JoinHandle<()>>,
}
impl ConsoleCloser {
    pub(crate) fn start() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Box<dyn MasterPty + Send>>(1);
        let thread = thread::Builder::new()
            .name("shellfox-pty-close".into())
            .spawn(move || {
                if let Ok(master) = rx.recv() {
                    drop(master);
                }
            })?;
        Ok(Self {
            tx,
            thread: Some(thread),
        })
    }
}

pub(crate) fn start_reaper() -> std::io::Result<SyncSender<Session>> {
    let (tx, rx) = mpsc::sync_channel::<Session>(1);
    thread::Builder::new()
        .name("shellfox-pty-reap".into())
        .spawn(move || {
            if let Ok(mut retained) = rx.recv() {
                while retained.shutdown().is_err() {
                    thread::sleep(Duration::from_secs(1));
                }
            }
        })?;
    Ok(tx)
}
