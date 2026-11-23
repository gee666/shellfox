use shellfox_runtime::Request;
use shellfox_service::native::NativeInputContext;

/// Frozen at UI outbox insertion. Neither the worker nor a receipt may replace
/// this marker/hint with the latest selected session or settlement state.
pub(in crate::terminal) struct Intent {
    pub request: Request,
    pub input: Option<NativeInputContext>,
}
