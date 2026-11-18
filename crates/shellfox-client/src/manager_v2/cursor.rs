use super::input_book::{AcceptedContext, InputBook};
pub use crate::v2_common::delivery::EventDisposition;
use crate::v2_common::delivery::{CursorError as DeliveryError, Stamp, Tracker};
use shellfox_protocol::{manager_v2::*, Validate};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorError {
    Delivery(DeliveryError),
    InvalidMessage,
    ControlRequired,
    ControlReleaseRequired,
    AlreadyControlled,
    IntrinsicReadOnly,
    LayoutUnavailable,
    Busy,
    DuplicateInput,
    UnknownInput,
    InvalidReceipt,
    OutcomeUnknown,
}
impl From<DeliveryError> for CursorError {
    fn from(v: DeliveryError) -> Self {
        Self::Delivery(v)
    }
}
/// A bounded native admission cursor, not a server grant issuer. Recovery must
/// be consumed AND its exact ACK accepted before interactive side effects.
pub struct ManagerCursor {
    tracker: Tracker<Binding>,
    recovery: Recovery,
    input: Option<ControlRef>,
    layout: Option<LayoutRef>,
    input_state: InputState,
    inputs: InputBook,
}
#[path = "cursor_recovery.rs"]
mod recovery;
impl ManagerCursor {
    pub fn control(&self, current: &Binding) -> Result<&ControlRef, CursorError> {
        self.tracker.ready_input(current)?;
        self.input.as_ref().ok_or(CursorError::ControlRequired)
    }
    pub fn observe(
        &mut self,
        event: &Event,
        current: &Binding,
    ) -> Result<EventDisposition, CursorError> {
        self.tracker.check(current)?;
        event.validate().map_err(|_| CursorError::InvalidMessage)?;
        self.tracker.reference(event.delivery())?;
        Ok(match event {
            Event::Data { sequence, .. } => self.tracker.data(*sequence)?,
            Event::SourceEnded { last_sequence, .. } => self.tracker.source_end(*last_sequence)?,
            Event::ProcessExited { last_sequence, .. } => self.tracker.process_exit(*last_sequence),
            Event::Revoked { .. } => {
                self.input = None;
                self.layout = None;
                self.tracker.revoke()
            }
            Event::Layout {
                layout_revision,
                cols,
                rows,
                last_sequence,
                ..
            } => self
                .tracker
                .layout(*layout_revision, *cols, *rows, Some(*last_sequence))?,
            Event::Error { error, .. } => {
                if matches!(error.detail, ErrorDetail::StaleLayout { .. }) {
                    self.tracker.require_recovery();
                }
                EventDisposition::Deliver
            }
        })
    }
    /// Freeze this owned request at FIFO admission. A deterministic pre-admission
    /// Full retains the SAME request/context; never call admit again to recapture.
    pub fn admit_input(
        &mut self,
        id: Id,
        input: Input,
        hints: InputHints,
        current: &Binding,
    ) -> Result<WriteTerminal, CursorError> {
        let control = self.control(current)?.clone();
        self.inputs.admit(id, &input, &hints)?;
        Ok(WriteTerminal {
            control,
            input_id: id,
            hints,
            input,
        })
    }
    pub fn admit_mouse(
        &mut self,
        id: Id,
        event: MouseEvent,
        cell_width_px: u32,
        cell_height_px: u32,
        hints: InputHints,
        current: &Binding,
    ) -> Result<WriteTerminal, CursorError> {
        let geometry = GeometryStamp {
            recovery_id: self.recovery.recovery_id(),
            layout_revision: self.recovery.layout_revision(),
            cols: self.recovery.cols(),
            rows: self.recovery.rows(),
            cell_width_px,
            cell_height_px,
        };
        self.admit_input(id, Input::Mouse { event, geometry }, hints, current)
    }
    /// An actual known receipt can be surfaced after source control loss without
    /// reinstating a capability. It contains no secret input or replacement ref.
    pub fn accepted(
        &mut self,
        receipt: AcceptedInput,
        current: &Binding,
    ) -> Result<AcceptedContext, CursorError> {
        self.tracker.binding(current)?;
        self.inputs.accepted(receipt)
    }
    pub fn input_unknown(&mut self, id: Id) -> Result<(), CursorError> {
        self.inputs.unknown(id)
    }
    pub fn cancel_before_send(&mut self, id: Id) -> Result<(), CursorError> {
        self.inputs.cancel_before_send(id)
    }
    pub fn pending_inputs(&self) -> usize {
        self.inputs.len()
    }
    pub fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        current: &Binding,
    ) -> Result<ResizeTerminal, CursorError> {
        let control = self.control(current)?.clone();
        let layout = self.layout.clone().ok_or(CursorError::LayoutUnavailable)?;
        let request = ResizeTerminal {
            control,
            layout,
            expected_layout_revision: self.recovery.layout_revision(),
            cols,
            rows,
        };
        request
            .validate()
            .map_err(|_| CursorError::InvalidMessage)?;
        if cols != self.recovery.cols() || rows != self.recovery.rows() {
            self.tracker.require_recovery();
        }
        Ok(request)
    }
    pub fn takeover_request(
        &mut self,
        current: &Binding,
    ) -> Result<TakeTerminalControl, CursorError> {
        self.tracker.check(current)?;
        if self.input.is_some() {
            return Err(CursorError::AlreadyControlled);
        }
        if self.input_state == InputState::IntrinsicReadOnly {
            return Err(CursorError::IntrinsicReadOnly);
        }
        let request = TakeTerminalControl {
            delivery: self.delivery().clone(),
            mode: TakeMode::Takeover,
        };
        self.tracker.revoke();
        self.input = None;
        self.layout = None;
        Ok(request)
    }
    pub fn recovery_request(&self, current: &Binding) -> Result<AttachTerminal, CursorError> {
        self.tracker.binding(current)?;
        Ok(AttachTerminal {
            tab_id: self.delivery().tab_id,
            resume: self.tracker.consumed.map(|after_sequence| Resume {
                generation: self.delivery().generation,
                after_sequence,
            }),
            control: ControlRequest::None,
        })
    }
    pub fn validate_detach(
        &self,
        request: &DetachTerminal,
        current: &Binding,
    ) -> Result<(), CursorError> {
        self.tracker.binding(current)?;
        if request.delivery != *self.delivery() {
            return Err(CursorError::InvalidMessage);
        }
        if request.release_control != self.input {
            return Err(CursorError::ControlReleaseRequired);
        }
        Ok(())
    }
    pub fn detach_request(
        &mut self,
        release_control: bool,
        current: &Binding,
    ) -> Result<DetachTerminal, CursorError> {
        self.tracker.binding(current)?;
        if self.input.is_some() && !release_control {
            return Err(CursorError::ControlReleaseRequired);
        }
        let request = DetachTerminal {
            delivery: self.delivery().clone(),
            release_control: if release_control {
                self.input.clone()
            } else {
                None
            },
        };
        self.tracker.revoke();
        self.input = None;
        self.layout = None;
        Ok(request)
    }
    pub fn begin_upload(
        &self,
        file_name: String,
        size: u64,
        current: &Binding,
    ) -> Result<BeginTerminalUpload, CursorError> {
        let request = BeginTerminalUpload {
            control: self.control(current)?.clone(),
            file_name,
            size,
        };
        request
            .validate()
            .map_err(|_| CursorError::InvalidMessage)?;
        Ok(request)
    }
}
