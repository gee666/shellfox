use anyhow::Result;
#[path = "raw_process_stdin_error.rs"]
mod stdin_error;
use std::sync::mpsc::{self, Receiver};
#[cfg(any(windows, target_os = "linux", test))]
pub(crate) use stdin_error::StdinFailure;
pub use stdin_error::{AutomationStdinClosed, AutomationStdinIoError, AutomationStdinUnavailable};
#[cfg(windows)]
#[path = "raw_process_workers.rs"]
mod workers;
#[cfg(windows)]
pub(super) use workers::Input;
#[cfg(any(windows, target_os = "linux"))]
#[path = "raw_process_output.rs"]
mod output;
#[cfg(any(windows, target_os = "linux"))]
pub(crate) use output::Output;

#[cfg(test)]
#[path = "raw_process_stdin_tests.rs"]
mod tests;

pub const CHUNK_BYTES: usize = 32768;
pub const INPUT_BYTES: usize = 49152;
pub const OUTPUT_CREDIT: usize = 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawStream {
    Stdout,
    Stderr,
}
pub enum RawOutput {
    Data { stream: RawStream, bytes: Vec<u8> },
    Eof(RawStream),
    Failed(RawStream),
}

impl std::fmt::Debug for RawOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Data { stream, bytes } => f
                .debug_struct("Data")
                .field("stream", stream)
                .field("bytes", &bytes.len())
                .finish(),
            Self::Eof(stream) => f.debug_tuple("Eof").field(stream).finish(),
            Self::Failed(stream) => f.debug_tuple("Failed").field(stream).finish(),
        }
    }
}
pub struct WriteReceipt {
    pub(crate) result: Receiver<Result<()>>,
}
impl WriteReceipt {
    pub fn try_result(&self) -> Result<Option<()>> {
        match self.result.try_recv() {
            Ok(result) => result.map(Some),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(AutomationStdinUnavailable::AcknowledgementLost.into())
            }
        }
    }
    pub fn wait(self) -> Result<()> {
        self.result
            .recv()
            .map_err(|_| AutomationStdinUnavailable::AcknowledgementLost)?
    }
}
