//! Shared bounded raw stdout/stderr queue. EOF follows all bytes per stream.
use super::{RawOutput, RawStream, CHUNK_BYTES, OUTPUT_CREDIT};
use crate::wake::{Wake, WakeSlot};
use anyhow::Result;
use std::{
    collections::VecDeque,
    io::Read,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};
#[derive(Default)]
struct Queue {
    frames: VecDeque<RawOutput>,
    bytes: usize,
    discard: bool,
}
#[derive(Clone, Default)]
pub(crate) struct Output {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    wake: WakeSlot,
}
impl Output {
    /// Called after every queued frame. Fires once now for frames queued earlier.
    pub fn set_wake(&self, wake: Wake) {
        self.wake.set(wake);
        self.wake.wake();
    }
    pub fn reader(
        &self,
        mut reader: impl Read + Send + 'static,
        stream: RawStream,
    ) -> Result<JoinHandle<()>> {
        let output = self.clone();
        Ok(std::thread::Builder::new()
            .name("shellfox-raw-output".into())
            .spawn(move || {
                let mut bytes = vec![0; CHUNK_BYTES];
                loop {
                    match reader.read(&mut bytes) {
                        Ok(0) => {
                            output.push(RawOutput::Eof(stream));
                            break;
                        }
                        Ok(count) => output.push(RawOutput::Data {
                            stream,
                            bytes: bytes[..count].to_vec(),
                        }),
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => {
                            output.push(RawOutput::Failed(stream));
                            break;
                        }
                    }
                }
            })?)
    }
    fn push(&self, frame: RawOutput) {
        let size = match &frame {
            RawOutput::Data { bytes, .. } => bytes.len(),
            _ => 0,
        };
        {
            let (queue, ready) = &*self.queue;
            let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
            while !queue.discard && (queue.bytes + size > OUTPUT_CREDIT || queue.frames.len() >= 64)
            {
                queue = ready.wait(queue).unwrap_or_else(|e| e.into_inner());
            }
            if queue.discard {
                return;
            }
            queue.bytes += size;
            queue.frames.push_back(frame);
        }
        self.wake.wake();
    }
    pub fn read(&self) -> Option<RawOutput> {
        let (queue, ready) = &*self.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        let frame = queue.frames.pop_front()?;
        if let RawOutput::Data { bytes, .. } = &frame {
            queue.bytes -= bytes.len();
        }
        ready.notify_all();
        Some(frame)
    }
    pub fn discard(&self) {
        let (queue, ready) = &*self.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.discard = true;
        queue.frames.clear();
        queue.bytes = 0;
        ready.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counter() -> (Arc<AtomicUsize>, Wake) {
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&wakes);
        (
            wakes,
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            }),
        )
    }

    #[test]
    fn every_frame_wakes_outside_the_queue_lock() {
        let output = Output::default();
        let (wakes, count) = counter();
        let reader = output.clone();
        // Reading from the wake would deadlock if it ran under the queue lock.
        output.set_wake(Arc::new(move || {
            count();
            while reader.read().is_some() {}
        }));
        assert_eq!(wakes.load(Ordering::SeqCst), 1, "registration wakes once");
        output
            .reader(&b"abc"[..], RawStream::Stdout)
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(wakes.load(Ordering::SeqCst), 3, "data and EOF each wake");
        assert!(output.read().is_none());
    }

    #[test]
    fn a_late_wake_fires_for_frames_already_queued() {
        let output = Output::default();
        output
            .reader(&b"abc"[..], RawStream::Stderr)
            .unwrap()
            .join()
            .unwrap();
        let (wakes, wake) = counter();
        output.set_wake(wake);
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        assert!(matches!(output.read(), Some(RawOutput::Data { .. })));
        assert!(matches!(
            output.read(),
            Some(RawOutput::Eof(RawStream::Stderr))
        ));
    }
}
