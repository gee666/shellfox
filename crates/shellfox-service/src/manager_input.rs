use super::*;
use crate::activity_integration::InputAdmission;
use shellfox_runtime::{Command, Response};
#[path = "manager_input_map.rs"]
mod mapping;
pub(super) fn write(
    state: &mut RemoteState,
    service: &mut Service,
    id: m::Id,
    request: m::WriteTerminal,
) -> Result<serde_json::Value, m::Error> {
    let input_id = request.input_id;
    let kind = request.input.kind();
    let (source, lease) =
        control::current(state, service, id, &request.control).map_err(application)?;
    let view = state
        .manager_views
        .entries
        .get_mut(&id)
        .ok_or_else(|| application(native_error()))?;
    if view.inputs.contains(&input_id) {
        return Err(uncertain(
            input_id,
            "Input ID was already admitted. Input was not replayed.",
        ));
    }
    // Bounded recent-ID tombstones. This is not an input archive or retry queue.
    if view.inputs.len() >= 256 {
        view.inputs.pop_front();
    }
    view.inputs.push_back(input_id);
    let captured = match request.hints.settlement_capture() {
        m::SettlementCapture::Unavailable => shellfox_host::CapturedSettlement::Unavailable,
        m::SettlementCapture::Captured(value) => {
            shellfox_host::CapturedSettlement::Captured(value.map(str::to_owned))
        }
    };
    let (command, admission) = match request.input {
        m::Input::Key { event } => (
            Command::KeyV2 {
                lease,
                event: mapping::key_v2(event).map_err(application)?,
            },
            InputAdmission::native(
                request.hints.user_initiated,
                request.hints.activity_hint,
                captured,
            ),
        ),
        m::Input::Paste { text } => (
            Command::Paste { lease, text },
            InputAdmission::native(
                request.hints.user_initiated,
                request.hints.activity_hint,
                captured,
            ),
        ),
        m::Input::Bytes { data, .. } => {
            let bytes =
                shellfox_protocol::observer_v2::decode_bytes(&data, m::MAX_ENCODED_INPUT_BYTES)
                    .map_err(|_| {
                        application(failure(
                            ErrorCode::Validation,
                            "Invalid canonical input bytes.",
                        ))
                    })?;
            let admission = InputAdmission::raw(
                &bytes,
                request.hints.user_initiated,
                request.hints.activity_hint,
                captured,
            )
            .map_err(|_| {
                application(failure(
                    ErrorCode::Validation,
                    "Input admission bound exceeded.",
                ))
            })?;
            (Command::InteractiveBytes { lease, bytes }, admission)
        }
        m::Input::Mouse { event, geometry } => {
            let stamp = service
                .manager_v2_mailboxes
                .get(source.identity, source.role)
                .ok_or_else(|| application(native_error()))?
                .geometry(&request.control, &geometry)
                .map_err(|_| stale())?;
            let event = mapping::mouse(event).map_err(application)?;
            (
                Command::MouseStamped {
                    lease,
                    event,
                    geometry: shellfox_ghostty_vt::MouseGeometry {
                        cell_width: geometry.cell_width_px,
                        cell_height: geometry.cell_height_px,
                    },
                    stamp,
                },
                InputAdmission::native(
                    request.hints.user_initiated,
                    request.hints.activity_hint,
                    captured,
                ),
            )
        }
    };
    let response=service.call_input(source.identity.tab_id,command,admission).map_err(|error|{
        if let Some(native)=error.downcast_ref::<shellfox_runtime::RuntimeError>(){
            return match native.code{
                shellfox_runtime::ErrorCode::ResourceLimit=>m::Error{code:ErrorCode::Validation,message:"Owner encoding exceeds the 64 KiB single-write bound; no input was written.".into(),retryable:false,detail:m::ErrorDetail::ResourceLimit{limit:m::Limit::EncodedInputBytes}},
                shellfox_runtime::ErrorCode::RecoveryRequired|shellfox_runtime::ErrorCode::StaleLayout=>stale(),
                shellfox_runtime::ErrorCode::Validation=>application(failure(ErrorCode::Validation,"Native owner rejected key/mouse/input fields before writing.")),
                shellfox_runtime::ErrorCode::ControlRequired|shellfox_runtime::ErrorCode::Closed=>application(failure(ErrorCode::TargetLost,"Actual controller lease or source is no longer ready.")),
                _=>uncertain(input_id,"Input write outcome is uncertain. Do not retry input."),
            };
        }
        uncertain(input_id,"Input admission/write outcome is uncertain. Do not retry input.")
    })?;
    let Response::Input(receipt) = response else {
        return Err(uncertain(
            input_id,
            "Runtime did not return an actual input receipt.",
        ));
    };
    let expected = match kind {
        m::InputKind::Bytes => matches!(receipt.kind, shellfox_runtime::InputKind::Raw),
        m::InputKind::Key => matches!(receipt.kind, shellfox_runtime::InputKind::Key { .. }),
        m::InputKind::Paste => matches!(receipt.kind, shellfox_runtime::InputKind::Paste { .. }),
        m::InputKind::Mouse => matches!(receipt.kind, shellfox_runtime::InputKind::Mouse { .. }),
    };
    if !expected {
        return Err(uncertain(
            input_id,
            "Native input kind did not match the admitted request.",
        ));
    }
    // Positive receipt is final even if control changed immediately afterward.
    // Service already finishes the ORIGINAL activity/CAS admission at Reply arrival.
    let result = m::AcceptedInput {
        input_id,
        kind,
        accepted_bytes: receipt.accepted_bytes,
        disposition: if receipt.accepted_bytes == 0 {
            m::Disposition::Ignored
        } else {
            m::Disposition::Accepted
        },
    };
    result.validate().map_err(|_| {
        uncertain(
            input_id,
            "Native encoded receipt exceeds its negotiated budget.",
        )
    })?;
    serde_json::to_value(result)
        .map_err(|_| uncertain(input_id, "Accepted input receipt encoding failed."))
}
fn application(error: AppError) -> m::Error {
    m::Error {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
        detail: m::ErrorDetail::Application,
    }
}
fn uncertain(input_id: m::Id, message: &str) -> m::Error {
    m::Error {
        code: ErrorCode::NativeUnavailable,
        message: message.into(),
        retryable: false,
        detail: m::ErrorDetail::InputOutcomeUnknown {
            input_id,
            retry_input: false,
        },
    }
}
fn stale() -> m::Error {
    m::Error {
        code: ErrorCode::TargetLost,
        message: "Consume full recovery for current stamped geometry before input.".into(),
        retryable: false,
        detail: m::ErrorDetail::StaleLayout {
            recovery_required: true,
        },
    }
}
