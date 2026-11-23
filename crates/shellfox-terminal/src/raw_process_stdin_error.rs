use std::{fmt, io, sync::Arc};

/// Known ended stdin or an actual OS BrokenPipe. This is never an input ACK or
/// process-exit/cleanup proof. Callers must correlate the final run separately.
#[derive(Clone, Debug, Default)]
pub struct AutomationStdinClosed {
    source: Option<Arc<io::Error>>,
}
impl AutomationStdinClosed {
    pub fn known() -> Self {
        Self::default()
    }
    pub fn io_error(&self) -> Option<&io::Error> {
        self.source.as_deref()
    }
}
impl fmt::Display for AutomationStdinClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The process stdin is closed.")
    }
}
impl std::error::Error for AutomationStdinClosed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.io_error().map(|error| error as _)
    }
}
/// Other OS write failures retain their exact kind/raw code and are not closed.
#[derive(Clone, Debug)]
pub struct AutomationStdinIoError(Arc<io::Error>);
impl AutomationStdinIoError {
    pub fn io_error(&self) -> &io::Error {
        &self.0
    }
}
impl fmt::Display for AutomationStdinIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Automation stdin write failed: {}", self.0)
    }
}
impl std::error::Error for AutomationStdinIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.io_error())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationStdinUnavailable {
    QueueFull,
    WriterDisconnected,
    AcknowledgementLost,
    WriterNotReady,
}
impl fmt::Display for AutomationStdinUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::QueueFull => "Automation stdin queue is full; input was not submitted.",
            Self::WriterDisconnected => {
                "Automation stdin writer channel disconnected; input was not submitted."
            }
            Self::AcknowledgementLost => {
                "Automation stdin writer stopped before acknowledgement; outcome is unconfirmed."
            }
            Self::WriterNotReady => "Automation stdin writer is not ready.",
        })
    }
}
impl std::error::Error for AutomationStdinUnavailable {}

#[derive(Clone)]
pub(crate) enum StdinFailure {
    Closed(AutomationStdinClosed),
    Io(AutomationStdinIoError),
}
impl StdinFailure {
    pub fn known_closed() -> Self {
        Self::Closed(AutomationStdinClosed::known())
    }
    pub fn from_io(error: io::Error) -> Self {
        let pipe = error.kind() == io::ErrorKind::BrokenPipe;
        let error = Arc::new(error);
        if pipe {
            Self::Closed(AutomationStdinClosed {
                source: Some(error),
            })
        } else {
            Self::Io(AutomationStdinIoError(error))
        }
    }
    pub fn into_error(self) -> anyhow::Error {
        match self {
            Self::Closed(error) => error.into(),
            Self::Io(error) => error.into(),
        }
    }
}
