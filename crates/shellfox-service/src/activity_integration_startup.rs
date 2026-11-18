//! Only actual Runtime::Event::Started results establish startup evidence.
use shellfox_protocol::{self as wire, Validate};

const FAILED: &str = "Owned terminal startup failed.";
#[derive(Clone, Default)]
pub enum StartupEvidence {
    #[default]
    Pending,
    Confirmed,
    Failed {
        sanitized_error: wire::AppError,
    },
}
impl StartupEvidence {
    /// Native/helper messages can contain executable arguments. Do not copy them.
    pub fn from_started(result: &Result<(), shellfox_runtime::RuntimeError>) -> Self {
        match result {
            Ok(()) => Self::Confirmed,
            Err(_) => Self::Failed {
                sanitized_error: wire::AppError {
                    code: wire::ErrorCode::LaunchFailed,
                    message: FAILED.into(),
                    retryable: true,
                },
            },
        }
    }
    pub(super) fn validate(&self) -> Result<(), wire::ValidationError> {
        if let Self::Failed { sanitized_error } = self {
            sanitized_error.validate()?;
            if !matches!(sanitized_error.code, wire::ErrorCode::LaunchFailed)
                || sanitized_error.message != FAILED
            {
                return Err(wire::ValidationError);
            }
        }
        Ok(())
    }
}
