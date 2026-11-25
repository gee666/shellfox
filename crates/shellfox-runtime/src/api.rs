use crate::RuntimeError;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_ghostty_vt::{
    key::KeyEvent, MouseEvent, MouseGeometry, Screen, Selection, TerminalColors, ViewportPoint,
};

pub const COMMAND_CAPACITY: usize = 16;
pub const EVENT_CAPACITY: usize = 32;
/// Client text/binary payload admission, before terminal-mode encoding.
pub const MAX_INPUT_PAYLOAD_BYTES: usize = 64 * 1024;
/// Complete encoded input must fit one existing bounded native write.
pub const MAX_ENCODED_INPUT_BYTES: usize = 64 * 1024;
/// Compatibility name for the original payload admission bound.
pub const MAX_INPUT_BYTES: usize = MAX_INPUT_PAYLOAD_BYTES;
/// Most key events in one `Command::Keys`.
pub const MAX_KEY_BATCH: usize = 256;
pub const MAX_SCREEN_CELLS: usize = 1024 * 1024;
pub const DEFAULT_SCREEN_CELLS: usize = 256 * 1024;
pub const MAX_SCREEN_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// The default covers high-DPI and ultrawide grids. The hard cell and UTF-8
/// aggregate caps match the checked Ghostty wrapper. Invalid resizes do not
/// mutate either the parser or PTY and never authorize process cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridLimits {
    max_cells: usize,
}
impl Default for GridLimits {
    fn default() -> Self {
        Self {
            max_cells: DEFAULT_SCREEN_CELLS,
        }
    }
}
impl GridLimits {
    pub fn new(max_cells: usize) -> Result<Self, RuntimeError> {
        if max_cells == 0 || max_cells > MAX_SCREEN_CELLS {
            return Err(RuntimeError::validation(
                "grid cell limit must be 1..=1048576",
            ));
        }
        Ok(Self { max_cells })
    }
    pub fn max_cells(self) -> usize {
        self.max_cells
    }
    pub fn validate_size(self, cols: u16, rows: u16) -> Result<(), RuntimeError> {
        if cols == 0
            || rows == 0
            || cols > i16::MAX as u16
            || rows > i16::MAX as u16
            || usize::from(cols) * usize::from(rows) > self.max_cells
        {
            return Err(RuntimeError::validation(format!(
                "terminal dimensions must be 1..=32767 and contain at most {} cells",
                self.max_cells
            )));
        }
        Ok(())
    }
}

/// Register this generation with the metadata host BEFORE starting the runtime.
/// This crate never writes metadata or commits a pending tab/session deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub session_id: SessionId,
    pub tab_id: TabId,
    pub generation: Generation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub controller: u64,
    pub epoch: u64,
}
/// Read-only delivery identity. It can never authorize input or resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserverLease {
    pub viewer: u64,
    pub epoch: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(pub u64);
#[derive(Debug)]
pub struct Request {
    pub id: RequestId,
    pub command: Command,
}
#[derive(Debug)]
pub enum Command {
    Claim {
        controller: u64,
        takeover: bool,
    },
    Presentation {
        view: crate::PresentationLease,
        action: crate::PresentationAction,
    },
    Observe {
        viewer: u64,
        after: Option<u64>,
    },
    ObserverReplay {
        observer: ObserverLease,
        after: u64,
    },
    ObserverAck {
        observer: ObserverLease,
        sequence: u64,
    },
    ObserverRecover {
        observer: ObserverLease,
        capability: AnsiRecoveryCapability,
    },
    ObserverRecoverStamped {
        observer: ObserverLease,
        capability: AnsiRecoveryCapability,
    },
    ObserverLayoutAck {
        observer: ObserverLease,
        recovery_id: crate::RecoveryId,
        last_delivery_sequence: u64,
        layout_revision: u64,
    },
    Unobserve {
        observer: ObserverLease,
    },
    /// Encoded by Ghostty with this terminal's live keyboard modes.
    Key {
        lease: Lease,
        event: KeyEvent,
    },
    /// Keys typed in a row, each encoded like `Key`, written in one PTY write.
    Keys {
        lease: Lease,
        events: Vec<KeyEvent>,
    },
    Paste {
        lease: Lease,
        text: String,
    },
    /// Local host/CLI bytes, never an implicit mobile or network capability.
    RawInput {
        lease: Lease,
        bytes: Vec<u8>,
    },
    Mouse {
        lease: Lease,
        event: MouseEvent,
        geometry: MouseGeometry,
    },
    Resize {
        lease: Lease,
        cols: u16,
        rows: u16,
    },
    Colors {
        lease: Lease,
        colors: Box<TerminalColors>,
    },
    Scroll {
        lease: Lease,
        delta: isize,
    },
    Selection {
        lease: Lease,
        selection: Option<Selection>,
    },
    Copy {
        lease: Lease,
    },
    Hyperlink {
        lease: Lease,
        point: ViewportPoint,
    },
    MouseTracking {
        lease: Lease,
    },
    Screen {
        lease: Lease,
    },
    Ack {
        lease: Lease,
        sequence: u64,
    },
    Replay {
        lease: Lease,
        after: u64,
    },
    /// Current visible cells as ANSI. Does not claim mode/scrollback restoration.
    /// Binary Ghostty snapshots are deliberately not a wire format.
    RecoverAnsi {
        lease: Lease,
    },
    /// Explicitly negotiated recovery fidelity. Neither capability is FullState.
    Recover {
        lease: Lease,
        capability: AnsiRecoveryCapability,
    },
    /// Opt this real controller epoch into full stamped recovery/layout fencing.
    RecoverStamped {
        lease: Lease,
        capability: AnsiRecoveryCapability,
    },
    ControllerLayoutAck {
        lease: Lease,
        recovery_id: crate::RecoveryId,
        last_delivery_sequence: u64,
        layout_revision: u64,
    },
    /// Interactive binary admission, not an exported local RawInput capability.
    InteractiveBytes {
        lease: Lease,
        bytes: Vec<u8>,
    },
    MouseStamped {
        lease: Lease,
        event: MouseEvent,
        geometry: MouseGeometry,
        stamp: crate::ControllerGeometryStamp,
    },
    /// Owner readiness check for external side effects; not a reusable grant.
    CheckControllerReady {
        lease: Lease,
    },
    /// Retire only this real lease. Never stop or close the original PTY.
    ReleaseControl {
        lease: Lease,
    },
    Close {
        lease: Lease,
    },
    /// Cleanup is host authority, independent of a disconnected view's lease.
    RetryClose,
}
impl Command {
    pub(crate) fn validate(&self, limits: GridLimits) -> Result<(), RuntimeError> {
        let bytes = match self {
            Self::Paste { text, .. } => text.len(),
            Self::RawInput { bytes, .. } | Self::InteractiveBytes { bytes, .. } => bytes.len(),
            Self::Key { event, .. } => event.text.len(),
            Self::Keys { events, .. } => {
                if events.is_empty() || events.len() > MAX_KEY_BATCH {
                    return Err(RuntimeError::validation("invalid key batch"));
                }
                events.iter().map(|event| event.text.len()).sum()
            }
            Self::Resize { cols, rows, .. } => return limits.validate_size(*cols, *rows),
            _ => 0,
        };
        if bytes > MAX_INPUT_PAYLOAD_BYTES {
            return Err(RuntimeError::validation("input exceeds 64 KiB"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Phase {
    Starting,
    Active,
    Draining,
    Closed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub sequence: u64,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenFrame {
    pub identity: Identity,
    pub sequence: u64,
    pub screen: Screen,
    pub mouse_tracking: bool,
    /// Wheel steps reach the application as cursor keys; see `Terminal::alternate_scroll`.
    #[serde(default)]
    pub alternate_scroll: bool,
}
/// Recovery fidelity is explicit, never inferred from the presence of ANSI bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiRecoveryCapability {
    VisibleScreen,
    FormattedActiveBufferV1,
    /// Bounded ANSI projection for xterm clients of the existing v1 protocol.
    RemoteV1,
}
#[derive(Debug)]
pub enum Response {
    Claimed(Lease),
    Observed(ObserverLease),
    Presentation(std::sync::Arc<crate::ViewFrame>),
    Input(crate::InputReceipt),
    Recovery(crate::StampedRecovery),
    Applied,
    Replay(Vec<Chunk>),
    Ansi {
        capability: AnsiRecoveryCapability,
        bytes: Vec<u8>,
        last_sequence: u64,
    },
    RemoteV1Ansi {
        bytes: Vec<u8>,
        last_sequence: u64,
        cols: u16,
        rows: u16,
    },
    Text(Option<String>),
    MouseTracking(bool),
}
