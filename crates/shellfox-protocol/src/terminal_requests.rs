use crate::{methods::method, validation::*, *};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachTerminal {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub after_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub generation: Option<Generation>,
}
impl Validate for AttachTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(value) = self.after_sequence {
            sequence(value)?;
        }
        Ok(())
    }
}
method!(AttachTerminal, TerminalAttachment, "attachTerminal");
macro_rules! identity_method {
    ($ty:ident, $response:ty, $name:literal) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $ty {
            #[serde(deserialize_with = "crate::ids::wire_id")]
            pub tab_id: TabId,
            #[serde(deserialize_with = "crate::ids::wire_id")]
            pub generation: Generation,
        }
        impl Validate for $ty {
            fn validate(&self) -> Result<(), ValidationError> {
                Ok(())
            }
        }
        method!($ty, $response, $name);
    };
}
identity_method!(TakeTerminalControl, Taken, "takeTerminalControl");
identity_method!(DetachTerminal, Detached, "detachTerminal");
identity_method!(CloseTab, SessionDto, "closeTab");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteTerminal {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub generation: Generation,
    pub control_epoch: u64,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub user_initiated: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    pub settled_at: Option<Option<String>>,
}
impl Validate for WriteTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.control_epoch)?;
        ensure(!self.data.is_empty() && self.data.len() <= 65536)?;
        if let Some(value) = &self.settled_at {
            optional_time(value)?;
        }
        Ok(())
    }
}
method!(WriteTerminal, Written, "writeTerminal");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResizeTerminal {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub generation: Generation,
    pub control_epoch: u64,
    pub cols: u16,
    pub rows: u16,
}
impl Validate for ResizeTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.control_epoch)?;
        ensure((2..=500).contains(&self.cols) && (2..=500).contains(&self.rows))
    }
}
method!(ResizeTerminal, Resized, "resizeTerminal");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcknowledgeTerminal {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub generation: Generation,
    pub control_epoch: u64,
    pub sequence: u64,
}
impl Validate for AcknowledgeTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.control_epoch)?;
        sequence(self.sequence)
    }
}
method!(AcknowledgeTerminal, Acknowledged, "acknowledgeTerminal");
