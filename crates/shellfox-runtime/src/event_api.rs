use crate::*;
#[derive(Debug)]
pub enum Event {
    Started(Result<(), RuntimeError>),
    Reply {
        id: RequestId,
        result: Result<Response, RuntimeError>,
    },
    Output {
        lease: Lease,
        chunk: Chunk,
    },
    ObserverOutput {
        observer: ObserverLease,
        chunk: Chunk,
    },
    ControllerLayout {
        lease: Lease,
        layout: crate::LayoutStamp,
    },
    ControllerRevoked {
        lease: Lease,
        reason: crate::ControllerRevocation,
    },
    ObserverLayout {
        observer: ObserverLease,
        layout: crate::LayoutStamp,
    },
    ObserverReplayLost {
        observer: ObserverLease,
        last_sequence: u64,
    },
    ObserverExited {
        observer: ObserverLease,
        last_sequence: u64,
    },
    /// This lease fell behind bounded replay. Explicit ANSI recovery is required.
    ReplayLost {
        lease: Lease,
        last_sequence: u64,
    },
    /// OwnedPty Closed confirms cleanup; ExternalBytes Closed ends only its
    /// passive view, NEVER proves the separate raw process exited/was cleaned.
    Phase {
        phase: Phase,
        error: Option<String>,
    },
    /// Delivered only after the final output sequence has entered the event queue.
    Exited {
        lease: Lease,
        last_sequence: u64,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Validation,
    ControlRequired,
    ReplayLost,
    StalePresentation,
    RecoveryRequired,
    ResourceLimit,
    StaleLayout,
    Unsupported,
    Unavailable,
    AdministratorStartup(shellfox_terminal::elevated::AdapterError),
    Closed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub code: ErrorCode,
    pub message: String,
}
impl RuntimeError {
    pub(crate) fn validation(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Validation,
            message: message.into(),
        }
    }
    pub(crate) fn unavailable(error: impl std::fmt::Display) -> Self {
        Self {
            code: ErrorCode::Unavailable,
            message: error.to_string(),
        }
    }
    pub(crate) fn control(message: String) -> Self {
        Self {
            code: ErrorCode::ControlRequired,
            message,
        }
    }
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for RuntimeError {}
