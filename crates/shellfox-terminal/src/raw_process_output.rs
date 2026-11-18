//! Shared bounded raw stdout/stderr queue. EOF follows all bytes per stream.
use super::{RawOutput, RawStream, CHUNK_BYTES, OUTPUT_CREDIT};
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
}
impl Output {
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
        let (queue, ready) = &*self.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        while !queue.discard && (queue.bytes + size > OUTPUT_CREDIT || queue.frames.len() >= 64) {
            queue = ready.wait(queue).unwrap_or_else(|e| e.into_inner());
        }
        if queue.discard {
            return;
        }
        queue.bytes += size;
        queue.frames.push_back(frame);
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
