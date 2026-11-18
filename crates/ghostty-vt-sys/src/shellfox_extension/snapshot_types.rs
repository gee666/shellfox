use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileState {
    pub abi_version: u32,
    pub terminal_profile: TerminalProfile,
    pub preceding_join: u32,
}
impl ProfileState {
    pub(in crate::shellfox_extension) fn checked(state: RawState) -> Result<Self> {
        if state.size != std::mem::size_of::<RawState>() || state.abi_version != 1 {
            return Err(Error::InvalidOutput("profile State ABI"));
        }
        match state.terminal_profile {
            0 => {}
            1 => return Err(Error::UnsupportedLegacy),
            _ => return Err(Error::InvalidOutput("unknown terminal profile")),
        }
        if state.preceding_join != 0 {
            return Err(Error::InvalidOutput("Native join State"));
        }
        Ok(Self {
            abi_version: 1,
            terminal_profile: TerminalProfile::Native,
            preceding_join: 0,
        })
    }
    pub(super) fn validate(self) -> Result<()> {
        if self.terminal_profile != TerminalProfile::Native {
            return Err(Error::UnsupportedLegacy);
        }
        if self.abi_version != 1 || self.preceding_join != 0 {
            return Err(Error::InvalidInput("Native State"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotLimits {
    pub max_encoded_bytes: usize,
    pub max_allocation_bytes: u64,
    pub max_work_units: u64,
    pub max_cells: u64,
    pub max_history_rows: u64,
    pub max_continuation_bytes: usize,
    pub max_columns: u32,
    pub max_rows: u32,
    pub max_pixel_width: u32,
    pub max_pixel_height: u32,
    pub max_page_rows: u32,
    pub max_pages: u32,
    pub max_records: u32,
}
impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: 4 * 1024 * 1024,
            max_allocation_bytes: 128 * 1024 * 1024,
            max_work_units: 100_000_000_000,
            max_cells: 1_048_576,
            max_history_rows: 10000,
            max_continuation_bytes: 65536,
            max_columns: 4096,
            max_rows: 4096,
            max_pixel_width: 65536,
            max_pixel_height: 65536,
            max_page_rows: 65535,
            max_pages: 1000,
            max_records: 10000,
        }
    }
}
impl SnapshotLimits {
    pub(super) fn validate(self) -> Result<()> {
        if self.max_encoded_bytes > 16 * 1024 * 1024
            || self.max_allocation_bytes > 512 * 1024 * 1024
            || self.max_work_units > 100_000_000_000
            || self.max_cells > 16 * 1024 * 1024
            || self.max_history_rows > 1_048_576
            || self.max_continuation_bytes > 65536
            || self.max_columns > 4096
            || self.max_rows > 4096
            || self.max_pixel_width > 65536
            || self.max_pixel_height > 65536
            || self.max_page_rows > 65535
            || self.max_pages > 65536
            || self.max_records > 1_048_576
        {
            return Err(Error::InvalidInput("snapshot binding hard limits"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub engine_identity: EngineIdentity,
    pub wire_profile: u32,
    pub library_version: u32,
    pub snapshot_version: u32,
    pub state: Option<ProfileState>,
    pub configuration: RestorationConfig,
}
impl SnapshotMetadata {
    pub(super) fn validate(&self, verified: VerifiedNativeExtension) -> Result<ProfileState> {
        if self.engine_identity != verified.identity {
            return Err(Error::IdentityMismatch);
        }
        self.validate_native0_raw_v1()
    }
    /// Shared codec/configuration admission, separate from strict build equality.
    /// Local Native002 ABI verification is required by both public entry types.
    pub(super) fn validate_native0_raw_v1(&self) -> Result<ProfileState> {
        if self.wire_profile != 1
            || self.library_version != 0x00010001
            || self.snapshot_version != 1
        {
            return Err(Error::InvalidInput("snapshot profile/version"));
        }
        self.configuration.validate()?;
        let state = self
            .state
            .ok_or(Error::InvalidInput("captured State is required"))?;
        state.validate()?;
        Ok(state)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotReport {
    pub source_offset: u64,
    pub heap_bytes: u64,
    pub mapped_bytes: u64,
    pub work_units: u64,
}
impl SnapshotReport {
    pub(super) fn checked(
        raw: RawReport,
        limits: SnapshotLimits,
        input_length: usize,
        encode: bool,
        success: bool,
    ) -> Result<Self> {
        if raw.size != std::mem::size_of::<RawReport>()
            || raw.abi_version != 1
            || raw.reserved != 0
            || raw
                .heap_bytes
                .checked_add(raw.mapped_bytes)
                .is_none_or(|bytes| bytes > limits.max_allocation_bytes)
            || raw.work_units > limits.max_work_units
            || raw.source_offset > input_length as u64
            || (encode && raw.source_offset != 0)
            || (!encode && success && raw.source_offset != input_length as u64)
        {
            return Err(Error::InvalidOutput("snapshot budget report"));
        }
        Ok(Self {
            source_offset: raw.source_offset,
            heap_bytes: raw.heap_bytes,
            mapped_bytes: raw.mapped_bytes,
            work_units: raw.work_units,
        })
    }
}
impl EncodeOptions {
    pub(super) fn build(
        verified: &VerifiedNativeExtension,
        limits: SnapshotLimits,
        config: RestorationConfig,
    ) -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            abi_version: 1,
            profile: 1,
            ghostty_version: 0x00010001,
            snapshot_version: 1,
            engine_profile: ByteString::from(&verified.identity.0),
            terminal_profile: 0,
            flags: 0,
            reserved: 0,
            max_encoded_bytes: limits.max_encoded_bytes as u64,
            max_allocation_bytes: limits.max_allocation_bytes,
            max_work_units: limits.max_work_units,
            max_cells: limits.max_cells,
            max_history_rows: limits.max_history_rows,
            max_continuation_bytes: limits.max_continuation_bytes as u64,
            max_image_bytes: 0,
            max_decompressed_bytes: 0,
            runtime_image_storage_limit: config.runtime_image_storage_limit,
            runtime_image_io_policy: 0,
            runtime_resize_pull_scrollback: u32::from(config.runtime_resize_pull_scrollback),
            max_columns: limits.max_columns,
            max_rows: limits.max_rows,
            max_pixel_width: limits.max_pixel_width,
            max_pixel_height: limits.max_pixel_height,
            max_page_rows: limits.max_page_rows,
            max_pages: limits.max_pages,
            max_records: limits.max_records,
            max_images: 0,
        }
    }
}
impl DecodeOptions {
    pub(super) fn build(
        verified: &VerifiedNativeExtension,
        limits: SnapshotLimits,
        config: RestorationConfig,
    ) -> Self {
        let x = EncodeOptions::build(verified, limits, config);
        Self {
            size: std::mem::size_of::<Self>(),
            abi_version: x.abi_version,
            profile: x.profile,
            ghostty_version: x.ghostty_version,
            snapshot_version: x.snapshot_version,
            engine_profile: ByteString::from(&verified.identity.0),
            profile_state: RawState::native(),
            flags: 0,
            reserved: 0,
            max_encoded_bytes: x.max_encoded_bytes,
            max_allocation_bytes: x.max_allocation_bytes,
            max_work_units: x.max_work_units,
            max_cells: x.max_cells,
            max_history_rows: x.max_history_rows,
            max_continuation_bytes: x.max_continuation_bytes,
            max_image_bytes: 0,
            max_decompressed_bytes: 0,
            runtime_image_storage_limit: x.runtime_image_storage_limit,
            runtime_image_io_policy: 0,
            runtime_resize_pull_scrollback: x.runtime_resize_pull_scrollback,
            max_columns: x.max_columns,
            max_rows: x.max_rows,
            max_pixel_width: x.max_pixel_width,
            max_pixel_height: x.max_pixel_height,
            max_page_rows: x.max_page_rows,
            max_pages: x.max_pages,
            max_records: x.max_records,
            max_images: 0,
        }
    }
}
