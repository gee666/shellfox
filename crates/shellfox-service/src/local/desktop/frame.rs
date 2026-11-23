use crate::native::NativeFrame;
use serde::{Deserialize, Serialize};
use shellfox_protocol::{validation::ensure, Validate, ValidationError};
use shellfox_runtime::{Phase, ViewFrame};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopFrame {
    frame_id: u64,
    presentation: Arc<ViewFrame>,
    highest_delivered: u64,
    recovery_barrier: Option<u64>,
    exited: Option<u64>,
    phase: Phase,
    error: Option<String>,
    replay_lost: bool,
    working_directory: Option<Arc<str>>,
}
impl From<NativeFrame> for DesktopFrame {
    fn from(frame: NativeFrame) -> Self {
        let NativeFrame {
            frame_id,
            presentation,
            highest_delivered,
            recovery_barrier,
            exited,
            phase,
            error,
            replay_lost,
            working_directory,
            ..
        } = frame;
        Self {
            frame_id,
            presentation,
            highest_delivered,
            recovery_barrier,
            exited,
            phase,
            error,
            replay_lost,
            working_directory,
        }
    }
}
impl From<DesktopFrame> for NativeFrame {
    fn from(frame: DesktopFrame) -> Self {
        let DesktopFrame {
            frame_id,
            presentation,
            highest_delivered,
            recovery_barrier,
            exited,
            phase,
            error,
            replay_lost,
            working_directory,
        } = frame;
        Self {
            frame_id,
            frame: presentation.screen.clone(),
            presentation,
            highest_delivered,
            recovery_barrier,
            exited,
            phase,
            error,
            replay_lost,
            working_directory,
        }
    }
}
impl Validate for DesktopFrame {
    fn validate(&self) -> Result<(), ValidationError> {
        let view = &self.presentation;
        let screen = &view.screen.screen;
        shellfox_runtime::GridLimits::default()
            .validate_size(screen.cols, screen.rows)
            .map_err(|_| ValidationError)?;
        ensure(
            self.frame_id > 0 && view.version > 0 && view.identity == view.screen.identity
            && view.sequence == view.screen.sequence
            // Immutable presentation can be newer than delivered raw output credit.
            && self.recovery_barrier.is_none_or(|barrier| view.sequence >= barrier)
            && screen.cells.len() == usize::from(screen.cols) * usize::from(screen.rows)
            && view.row_metadata.len() == usize::from(screen.rows)
            && view.top_row.saturating_add(u64::from(screen.rows)) <= view.total_rows
            && screen.cells.iter().all(|cell| cell.width <= 2 && cell.text.len() <= 1024),
        )?;
        if let Some(path) = &self.working_directory {
            shellfox_core::validation::cwd(path).map_err(|_| ValidationError)?;
        }
        Ok(())
    }
}
