//! Runtime errors a view reacts to (take control elsewhere, recover, refetch)
//! keep their meaning across the private transport, so a separate host process
//! doesn't turn them into a generic failure that stops the view.
use shellfox_protocol::ErrorCode;
use shellfox_runtime::{ErrorCode as RuntimeCode, RuntimeError};

const MAPPED: [(RuntimeCode, ErrorCode); 3] = [
    (
        RuntimeCode::ControlRequired,
        ErrorCode::TerminalControlRequired,
    ),
    (RuntimeCode::ReplayLost, ErrorCode::ReplayLost),
    (RuntimeCode::StalePresentation, ErrorCode::StalePresentation),
];

/// The wire code and message for the first mapped runtime error in the chain.
pub(super) fn to_wire(error: &anyhow::Error) -> Option<(ErrorCode, String)> {
    let runtime = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<RuntimeError>())?;
    let (_, code) = MAPPED.iter().find(|(from, _)| *from == runtime.code)?;
    Some((*code, runtime.message.clone()))
}

pub(super) fn from_wire(error: &shellfox_protocol::AppError) -> Option<RuntimeError> {
    let (code, _) = MAPPED.iter().find(|(_, to)| *to == error.code)?;
    Some(RuntimeError {
        code: *code,
        message: error.message.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_errors_survive_the_transport() {
        let rejected = anyhow::Error::new(RuntimeError {
            code: RuntimeCode::ControlRequired,
            message: "Terminal control belongs to another controller.".into(),
        })
        .context("Runtime rejected ownership without a new grant.");
        let (code, message) = to_wire(&rejected).expect("mapped");
        assert_eq!(code, ErrorCode::TerminalControlRequired);
        let restored = from_wire(&shellfox_protocol::AppError {
            code,
            message,
            retryable: false,
        })
        .expect("restored");
        assert_eq!(restored.code, RuntimeCode::ControlRequired);
        assert_eq!(
            restored.message,
            "Terminal control belongs to another controller."
        );
        assert!(to_wire(&anyhow::Error::new(RuntimeError {
            code: RuntimeCode::Unavailable,
            message: "x".into()
        }))
        .is_none());
        assert!(to_wire(&anyhow::anyhow!("plain")).is_none());
    }
}
