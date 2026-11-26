//! The Host works on an in-memory copy of its store. Temporary triggers on the
//! copy record every row a write transaction changes, and the journal replays
//! those rows on the disk file, statement for statement.
//!
//! Write-through (the default) replays a transaction on disk before the memory
//! transaction commits, so a failed disk write rolls the change back and the
//! call fails, exactly like a direct disk transaction.
//!
//! Journaled mode (`Host::start_journal`) commits in memory at once and hands
//! the rows to a writer thread, which replays them on disk strictly in order.
//! A disk that stalls or fails only delays durability: the writer retries the
//! same changes until they commit and logs while it can't. Callers wait for
//! `durable() >= seq` before they report a change as saved.
use crate::storage::storage_error;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use shellfox_core::{Error, ErrorCode, Result};
mod capture;
use capture::{Change, Schema};
use std::{
    sync::{mpsc, Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Called with a change's sequence number: `committed` on the writing thread
/// right after the memory commit, `durable` on the writer thread once the disk
/// commit returned. Neither may call back into the Host.
pub type JournalHook = Arc<dyn Fn(u64) + Send + Sync>;

#[derive(Clone)]
pub struct JournalHooks {
    pub committed: JournalHook,
    pub durable: JournalHook,
}

/// A disk write that keeps failing is logged again after this long.
const LOG_EVERY: Duration = Duration::from_secs(30);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(2);
struct Progress {
    durable: u64,
    /// The disk error the writer is retrying, while it fails.
    failure: Option<String>,
}
struct Shared {
    schema: Schema,
    progress: Mutex<Progress>,
    changed: Condvar,
}
impl Shared {
    fn progress(&self) -> std::sync::MutexGuard<'_, Progress> {
        self.progress
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
    fn durable(&self, seq: u64) {
        let mut progress = self.progress();
        progress.durable = seq;
        progress.failure = None;
        self.changed.notify_all();
    }
}

enum Mode {
    WriteThrough(Connection),
    Journaled {
        queue: Option<mpsc::Sender<(u64, Vec<Change>)>>,
        writer: Option<JoinHandle<()>>,
        committed: JournalHook,
    },
    /// Only while switching modes.
    Switching,
}

pub(crate) struct Journal {
    mode: Mode,
    written: u64,
    shared: Arc<Shared>,
}
impl Journal {
    /// Installs the capture triggers on `memory`, the copy of `disk`.
    pub fn new(disk: Connection, memory: &Connection) -> Result<Self> {
        let schema = Schema::read(&disk)?;
        schema.install(memory)?;
        Ok(Self {
            mode: Mode::WriteThrough(disk),
            written: 0,
            shared: Arc::new(Shared {
                schema,
                progress: Mutex::new(Progress {
                    durable: 0,
                    failure: None,
                }),
                changed: Condvar::new(),
            }),
        })
    }
    pub fn written(&self) -> u64 {
        self.written
    }
    pub fn durable(&self) -> u64 {
        self.shared.progress().durable
    }
    pub fn failure(&self) -> Option<String> {
        self.shared.progress().failure.clone()
    }
    pub fn start(&mut self, hooks: JournalHooks) -> Result<()> {
        if !matches!(self.mode, Mode::WriteThrough(_)) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "The metadata journal is already running.",
            ));
        }
        let Mode::WriteThrough(disk) = std::mem::replace(&mut self.mode, Mode::Switching) else {
            return Err(storage_error("journal mode"));
        };
        let (queue, entries) = mpsc::channel();
        let shared = self.shared.clone();
        let durable = hooks.durable;
        let writer = match std::thread::Builder::new()
            .name("shellfox-metadata-writer".into())
            .spawn(move || write_loop(disk, entries, &shared, &durable))
        {
            Ok(writer) => writer,
            Err(error) => {
                // The disk connection moved into the failed closure; keep the
                // switching state so writes fail instead of losing durability.
                return Err(storage_error(error));
            }
        };
        self.mode = Mode::Journaled {
            queue: Some(queue),
            writer: Some(writer),
            committed: hooks.committed,
        };
        Ok(())
    }
    /// Waits until everything written is on disk, at most `timeout`.
    pub fn flush(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut progress = self.shared.progress();
        while progress.durable < self.written {
            let now = Instant::now();
            if now >= deadline {
                let cause = progress.failure.as_deref().unwrap_or("the disk is slow");
                return Err(Error::new(
                    ErrorCode::StorageFailed,
                    &format!("Metadata is not saved to disk yet ({cause})."),
                ));
            }
            progress = self
                .shared
                .changed
                .wait_timeout(progress, deadline - now)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        Ok(())
    }
}
impl Drop for Journal {
    /// The store lock is released after this, so every queued change reaches
    /// the disk first, however long that takes.
    fn drop(&mut self) {
        if let Mode::Journaled { queue, writer, .. } = &mut self.mode {
            queue.take();
            if let Some(writer) = writer.take() {
                let _ = writer.join();
            }
        }
    }
}

/// One write transaction on the memory copy.
pub(crate) struct Write<'a> {
    tx: Transaction<'a>,
    journal: &'a mut Journal,
}
impl<'a> Write<'a> {
    pub fn begin(db: &'a Connection, journal: &'a mut Journal) -> Result<Self> {
        let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        Ok(Self { tx, journal })
    }
    /// Commits the change (see the module comment for the two modes). A
    /// transaction that changed no row commits without a journal entry.
    pub fn commit(self) -> Result<()> {
        let Self { tx, journal } = self;
        let changes = journal.shared.schema.take(&tx)?;
        if changes.is_empty() {
            return tx.commit().map_err(storage_error);
        }
        let seq = journal.written + 1;
        match &mut journal.mode {
            Mode::WriteThrough(disk) => {
                // On error the memory transaction drops uncommitted: rolled back.
                journal
                    .shared
                    .schema
                    .apply(disk, &[changes])
                    .map_err(storage_error)?;
                tx.commit().map_err(storage_error)?;
                journal.written = seq;
                journal.shared.durable(seq);
            }
            Mode::Journaled {
                queue, committed, ..
            } => {
                let queue = queue
                    .as_ref()
                    .ok_or_else(|| storage_error("journal closed"))?;
                tx.commit().map_err(storage_error)?;
                journal.written = seq;
                // The writer lives until Drop, so the send can't fail before.
                let _ = queue.send((seq, changes));
                committed(seq);
            }
            Mode::Switching => return Err(storage_error("journal unavailable")),
        }
        Ok(())
    }
}
impl std::ops::Deref for Write<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.tx
    }
}

fn write_loop(
    disk: Connection,
    entries: mpsc::Receiver<(u64, Vec<Change>)>,
    shared: &Shared,
    durable: &JournalHook,
) {
    while let Ok(first) = entries.recv() {
        // Everything queued meanwhile goes in the same disk transaction.
        let mut batch = vec![first];
        batch.extend(entries.try_iter());
        let last = batch.last().map_or(0, |(seq, _)| *seq);
        let changes: Vec<_> = batch.into_iter().map(|(_, changes)| changes).collect();
        let mut delay = Duration::from_millis(50);
        let mut failing: Option<(Instant, Instant)> = None;
        while let Err(error) = shared.schema.apply(&disk, &changes) {
            let now = Instant::now();
            let (since, logged) = failing.unwrap_or((now, now - LOG_EVERY));
            let logged = if now - logged >= LOG_EVERY {
                eprintln!(
                    "Shellfox host: metadata can't be saved to disk for {} s: {error}. Still retrying; changes wait in memory.",
                    (now - since).as_secs()
                );
                now
            } else {
                logged
            };
            failing = Some((since, logged));
            shared.progress().failure = Some(error.to_string());
            std::thread::sleep(delay);
            delay = (delay * 2).min(MAX_RETRY_DELAY);
        }
        if let Some((since, _)) = failing {
            eprintln!(
                "Shellfox host: metadata is saved to disk again after {} s.",
                since.elapsed().as_secs()
            );
        }
        shared.durable(last);
        durable(last);
    }
}
