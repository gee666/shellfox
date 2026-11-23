use super::{SystemResult, SystemScope};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistrySlot {
    UserPath,
    SystemPath,
    Explorer {
        scope: SystemScope,
        background: bool,
    },
    LegacyExplorer {
        scope: SystemScope,
        background: bool,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryField {
    Path,
    Owner,
    Label,
    MenuLabel,
    Icon,
    Selection,
    Command,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryStringKind {
    Plain,
    Expandable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryText {
    pub value: String,
    pub kind: RegistryStringKind,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistryRevision {
    pub key: u64,
    pub command: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistryState {
    pub exists: bool,
    pub revision: RegistryRevision,
    pub values: BTreeMap<RegistryField, RegistryText>,
    pub foreign_content: bool,
}
impl RegistryState {
    pub fn text(&self, field: RegistryField) -> Option<&str> {
        self.values.get(&field).map(|value| value.value.as_str())
    }
}
/// Slots are fixed by the host implementation. No request supplies a raw hive,
/// registry path, value name, command source or shell script.
pub trait SystemRegistry: Send + Sync {
    fn read(&self, slot: RegistrySlot) -> SystemResult<RegistryState>;
    fn compare_replace(
        &self,
        slot: RegistrySlot,
        expected: &RegistryState,
        desired: Option<&RegistryState>,
    ) -> SystemResult<()>;
}

#[cfg(not(windows))]
pub struct NativeSystemRegistry;
#[cfg(not(windows))]
impl SystemRegistry for NativeSystemRegistry {
    fn read(&self, _: RegistrySlot) -> SystemResult<RegistryState> {
        Err(super::error(
            shellfox_protocol::ErrorCode::Unsupported,
            "Windows registry integration is unavailable on this OS.",
        ))
    }
    fn compare_replace(
        &self,
        _: RegistrySlot,
        _: &RegistryState,
        _: Option<&RegistryState>,
    ) -> SystemResult<()> {
        Err(super::error(
            shellfox_protocol::ErrorCode::Unsupported,
            "Windows registry integration is unavailable on this OS.",
        ))
    }
}
