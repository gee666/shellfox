use crate::AnsiRecoveryCapability;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryId {
    pub(crate) domain: u64,
    pub(crate) value: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRevocation {
    Replaced,
    Released,
}
/// Captured at admission. Presentation metrics remain in MouseGeometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerGeometryStamp {
    pub recovery_id: RecoveryId,
    pub layout_revision: u64,
    pub cols: u16,
    pub rows: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutStamp {
    pub cols: u16,
    pub rows: u16,
    pub layout_revision: u64,
    /// Last byte-delivery sequence parsed before this canonical geometry change.
    pub last_delivery_sequence: u64,
}
/// One real lease-bound recovery cut, issued only by the existing layout fence.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryCut {
    pub recovery_id: RecoveryId,
    pub last_delivery_sequence: u64,
    pub cols: u16,
    pub rows: u16,
    pub layout_revision: u64,
}
#[cfg(feature = "experimental-shellfox-native")]
#[derive(Debug)]
pub struct StampedNative0Recovery {
    pub snapshot: shellfox_ghostty_vt::extension::NativeSnapshot,
    pub limits: shellfox_ghostty_vt::extension::SnapshotLimits,
    pub cut: RecoveryCut,
}
#[derive(Debug)]
pub struct StampedRecovery {
    pub bytes: Vec<u8>,
    pub capability: AnsiRecoveryCapability,
    pub last_delivery_sequence: u64,
    pub cols: u16,
    pub rows: u16,
    pub layout_revision: u64,
    /// Server-side correlation for this exact full snapshot. Not layout authority.
    pub recovery_id: RecoveryId,
}
