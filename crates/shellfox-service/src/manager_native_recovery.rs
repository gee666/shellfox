//! Actual Native0 response mapping only; no stamp/lease/ACK is manufactured here.
use super::*;
impl ManagerDelivery {
    pub fn recover_native0(
        &mut self,
        response: Response,
        selection: &wire::PairSelection,
    ) -> Result<wire::Recovery, Failure> {
        if selection.recovery_format != wire::RecoveryFormat::Native0RawV1 {
            return Err(Failure::Invalid);
        }
        let Response::Native0Recovery(native) = response else {
            return Err(Failure::Invalid);
        };
        let metadata = native.snapshot.metadata();
        let identity = String::from_utf8(metadata.engine_identity.as_bytes().to_vec())
            .map_err(|_| Failure::Invalid)?;
        let codec = wire::Native0Codec::new(
            wire::EngineIdentity::try_from(identity).map_err(|_| Failure::Invalid)?,
        );
        if selection.native0_sender.as_ref() != Some(&codec) {
            return Err(Failure::Stale);
        }
        if metadata.wire_profile != 1
            || metadata.library_version != 0x00010001
            || metadata.snapshot_version != 1
        {
            return Err(Failure::Invalid);
        }
        let state = metadata.state.ok_or(Failure::Invalid)?;
        if state.terminal_profile != shellfox_ghostty_vt::extension::TerminalProfile::Native {
            return Err(Failure::Invalid);
        }
        let limits = native.limits;
        let limits = wire::Native0Limits {
            max_encoded_bytes: u32::try_from(limits.max_encoded_bytes)
                .map_err(|_| Failure::Bounds)?,
            max_allocation_bytes: limits.max_allocation_bytes,
            max_work_units: limits.max_work_units,
            max_cells: limits.max_cells,
            max_history_rows: limits.max_history_rows,
            max_continuation_bytes: u32::try_from(limits.max_continuation_bytes)
                .map_err(|_| Failure::Bounds)?,
            max_columns: limits.max_columns,
            max_rows: limits.max_rows,
            max_pixel_width: limits.max_pixel_width,
            max_pixel_height: limits.max_pixel_height,
            max_page_rows: limits.max_page_rows,
            max_pages: limits.max_pages,
            max_records: limits.max_records,
        };
        let metadata = wire::Native0Metadata {
            codec,
            state: wire::Native0State {
                abi_version: state.abi_version,
                terminal_profile: 0,
                preceding_join: state.preceding_join,
            },
            configuration: wire::Native0Configuration {
                runtime_image_storage_limit: metadata.configuration.runtime_image_storage_limit,
                runtime_resize_pull_scrollback: metadata
                    .configuration
                    .runtime_resize_pull_scrollback,
            },
            limits,
        };
        let cut = native.cut;
        let recovery = wire::Recovery::Native0RawV1 {
            recovery_id: wire::Id::new(),
            encoding: wire::Encoding::Base64,
            data: base64(native.snapshot.as_bytes())?,
            last_sequence: cut.last_delivery_sequence,
            cols: cut.cols,
            rows: cut.rows,
            layout_revision: cut.layout_revision,
            metadata,
        };
        recovery
            .check_selection(selection)
            .map_err(|_| Failure::Invalid)?;
        self.commit_recovery(recovery, cut)
    }
}
