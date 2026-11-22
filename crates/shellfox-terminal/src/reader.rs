use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::wake::{Wake, WakeSlot};
use crate::{PtyOutput, OUTPUT_CHUNK_BYTES, OUTPUT_QUEUE_CHUNKS};

pub(crate) struct OutputReader {
    rx: Receiver<PtyOutput>,
    cancel: Arc<AtomicBool>,
    wake: WakeSlot,
    thread: Option<JoinHandle<()>>,
}

impl OutputReader {
    pub fn start(reader: Box<dyn Read + Send>) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(OUTPUT_QUEUE_CHUNKS);
        let cancel = Arc::new(AtomicBool::new(false));
        let wake = WakeSlot::default();
        let (stop, notify) = (cancel.clone(), wake.clone());
        let thread = thread::Builder::new()
            .name("shellfox-pty-read".into())
            .spawn(move || pump(reader, tx, &stop, &notify))?;
        Ok(Self {
            rx,
            cancel,
            wake,
            thread: Some(thread),
        })
    }

    /// Called after every queued item. Fires once now for output queued earlier.
    pub fn set_wake(&self, wake: Wake) {
        self.wake.set(wake);
        self.wake.wake();
    }

    pub fn try_read(&self) -> Result<Option<PtyOutput>, TryRecvError> {
        match self.rx.try_recv() {
            Ok(output) => Ok(Some(output)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error),
        }
    }

    // Keep draining the OS pipe after cancellation so ClosePseudoConsole cannot
    // deadlock behind a full output queue. Explicit shutdown may discard output.
    // Emptying the queue releases a blocked send; later reads are discarded.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        while self.rx.try_recv().is_ok() {}
    }

    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn join(&mut self) -> anyhow::Result<()> {
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("PTY reader panicked"))?;
        }
        Ok(())
    }
}

fn send(tx: &SyncSender<PtyOutput>, event: PtyOutput, wake: &WakeSlot) {
    if tx.send(event).is_ok() {
        wake.wake();
    }
}

fn pump(
    mut reader: Box<dyn Read + Send>,
    tx: SyncSender<PtyOutput>,
    cancel: &AtomicBool,
    wake: &WakeSlot,
) {
    let mut buffer = [0; OUTPUT_CHUNK_BYTES];
    loop {
        let event = match reader.read(&mut buffer) {
            Ok(0) => PtyOutput::Eof,
            Ok(count) => {
                if !cancel.load(Ordering::Acquire) {
                    send(&tx, PtyOutput::Data(buffer[..count].to_vec()), wake);
                }
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => PtyOutput::Eof,
            Err(error) => PtyOutput::ReadError(error),
        };
        send(&tx, event, wake);
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::{Duration, Instant};

    #[test]
    fn bounded_queue_preserves_final_bytes_before_eof() {
        let bytes = vec![b'x'; OUTPUT_CHUNK_BYTES * (OUTPUT_QUEUE_CHUNKS + 7)];
        let mut reader = OutputReader::start(Box::new(Cursor::new(bytes.clone()))).unwrap();
        thread::sleep(Duration::from_millis(30));
        assert!(
            !reader.finished(),
            "producer should wait at the queue limit"
        );
        let mut result = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(Instant::now() < deadline);
            match reader.try_read() {
                Ok(Some(PtyOutput::Data(chunk))) => {
                    assert!(chunk.len() <= OUTPUT_CHUNK_BYTES);
                    result.extend(chunk);
                }
                Ok(Some(PtyOutput::Eof)) => break,
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                other => panic!("unexpected output: {other:?}"),
            }
        }
        reader.join().unwrap();
        assert_eq!(result, bytes);
    }

    #[test]
    fn cancellation_releases_a_full_queue_without_a_consumer() {
        let bytes = vec![0; OUTPUT_CHUNK_BYTES * (OUTPUT_QUEUE_CHUNKS + 2)];
        let mut reader = OutputReader::start(Box::new(Cursor::new(bytes))).unwrap();
        thread::sleep(Duration::from_millis(30));
        reader.cancel();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !reader.finished() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        reader.join().unwrap();
    }
}
