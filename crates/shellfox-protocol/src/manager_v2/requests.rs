use super::*;
use crate::{validation::ensure, Validate, ValidationError};
use serde::{de::DeserializeOwned, Serialize};
mod sealed {
    pub trait Sealed {}
}
pub trait Request: sealed::Sealed + Serialize + DeserializeOwned + Validate {
    type Response: Serialize + DeserializeOwned + Validate;
    const METHOD: &'static str;
    fn validate_response(&self, _value: &Self::Response) -> Result<(), ValidationError> {
        Ok(())
    }
    fn validate_wire(&self) -> Result<(), ValidationError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| ValidationError)?;
        ensure(bytes.len() <= crate::MAX_REQUEST_BYTES)?;
        let _: Self = serde_json::from_slice(&bytes).map_err(|_| ValidationError)?;
        Ok(())
    }
}
macro_rules! compatible {
    ($name:ident) => {
        pub use crate::observer_v2::$name;
        impl sealed::Sealed for $name {}
        impl Request for $name {
            type Response = <$name as crate::observer_v2::Request>::Response;
            const METHOD: &'static str = <$name as crate::observer_v2::Request>::METHOD;
        }
    };
}
compatible!(GetSnapshot);
compatible!(GetTerminalProfiles);
compatible!(GetHomeDirectory);
compatible!(CreateSession);
compatible!(AddTab);
compatible!(ActivateSession);
compatible!(RenameSession);
compatible!(RenameTab);
compatible!(ReorderTabs);
compatible!(SetSessionPinned);
compatible!(SettleSession);
compatible!(UnsettleSession);
compatible!(DeleteSession);
compatible!(ClearSessionError);
compatible!(SetSessionEnv);
compatible!(SetTerminalEnv);
compatible!(SetVisibleSessions);
compatible!(CloseTab);
compatible!(ListFiles);
compatible!(ReadFile);
compatible!(WriteFile);
compatible!(BeginFileRead);
compatible!(ReadFileChunk);
compatible!(BeginFileWrite);
compatible!(WriteFileChunk);
compatible!(FinishFileWrite);
compatible!(CancelFileTransfer);
compatible!(WriteImageFile);
compatible!(ReadFileAsset);
compatible!(GetSessionStats);
compatible!(ResolveDirectory);
compatible!(CreateDirectory);
compatible!(CompleteDirectory);
macro_rules! own {
    ($name:ident,$response:ty,$method:literal) => {
        impl sealed::Sealed for $name {}
        impl Request for $name {
            type Response = $response;
            const METHOD: &'static str = $method;
        }
    };
}
own!(AttachTerminal, Attachment, "attachTerminal");
own!(TakeTerminalControl, Attachment, "takeTerminalControl");
impl sealed::Sealed for WriteTerminal {}
impl Request for WriteTerminal {
    type Response = AcceptedInput;
    const METHOD: &'static str = "writeTerminal";
    fn validate_response(&self, value: &AcceptedInput) -> Result<(), ValidationError> {
        value.check_input(self)
    }
}
impl sealed::Sealed for ResizeTerminal {}
impl Request for ResizeTerminal {
    type Response = Resized;
    const METHOD: &'static str = "resizeTerminal";
    fn validate_response(&self, value: &Resized) -> Result<(), ValidationError> {
        ensure(
            value.cols == self.cols
                && value.rows == self.rows
                && value.layout_revision >= self.expected_layout_revision,
        )
    }
}
own!(
    AcknowledgeTerminal,
    crate::Acknowledged,
    "acknowledgeTerminal"
);
own!(DetachTerminal, crate::Detached, "detachTerminal");
own!(
    BeginTerminalUpload,
    TerminalUploadStarted,
    "beginTerminalUpload"
);
own!(
    WriteTerminalUpload,
    crate::TransferOffset,
    "writeTerminalUpload"
);
own!(
    FinishTerminalUpload,
    crate::TerminalUploadResult,
    "finishTerminalUpload"
);
own!(
    CancelTerminalUpload,
    crate::Cancelled,
    "cancelTerminalUpload"
);
