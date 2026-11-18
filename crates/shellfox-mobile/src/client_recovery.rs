use super::*;

#[cfg(feature = "experimental-shellfox-native")]
pub(crate) fn decode_native0(
    recovery: &shellfox_protocol::manager_v2::Recovery,
    selection: &shellfox_protocol::manager_v2::PairSelection,
    receiver: &shellfox_ghostty_vt::extension::VerifiedNativeExtension,
    local: shellfox_ghostty_vt::extension::SnapshotLimits,
) -> std::result::Result<shellfox_ghostty_vt::Terminal, crate::Status> {
    use shellfox_ghostty_vt::extension as vt;
    use shellfox_protocol::manager_v2 as m;
    recovery
        .check_selection(selection)
        .map_err(|_| crate::Status::Invalid)?;
    let m::Recovery::Native0RawV1 { metadata, .. } = recovery else {
        return Err(crate::Status::Invalid);
    };
    let expected_sender = selection
        .native0_sender
        .as_ref()
        .ok_or(crate::Status::Invalid)?;
    let expected_receiver = selection
        .native0_receiver
        .as_ref()
        .ok_or(crate::Status::Invalid)?;
    if expected_receiver.engine_identity.as_str().as_bytes() != receiver.identity().as_bytes() {
        return Err(crate::Status::Stale);
    }
    let source = &metadata.limits;
    let limits = vt::SnapshotLimits {
        max_encoded_bytes: local
            .max_encoded_bytes
            .min(usize::try_from(source.max_encoded_bytes).map_err(|_| crate::Status::Invalid)?)
            .min(crate::MAX_QUEUE),
        max_allocation_bytes: local.max_allocation_bytes.min(source.max_allocation_bytes),
        max_work_units: local.max_work_units.min(source.max_work_units),
        max_cells: local.max_cells.min(source.max_cells).min(65536),
        max_history_rows: local.max_history_rows.min(source.max_history_rows),
        max_continuation_bytes: local.max_continuation_bytes.min(
            usize::try_from(source.max_continuation_bytes).map_err(|_| crate::Status::Invalid)?,
        ),
        max_columns: local.max_columns.min(source.max_columns).min(512),
        max_rows: local.max_rows.min(source.max_rows).min(512),
        max_pixel_width: local.max_pixel_width.min(source.max_pixel_width),
        max_pixel_height: local.max_pixel_height.min(source.max_pixel_height),
        max_page_rows: local.max_page_rows.min(source.max_page_rows),
        max_pages: local.max_pages.min(source.max_pages),
        max_records: local.max_records.min(source.max_records),
    };
    let sender = vt::EngineIdentity::from_hex(expected_sender.engine_identity.as_str())
        .map_err(|_| crate::Status::Invalid)?;
    let claimed = vt::EngineIdentity::from_hex(metadata.codec.engine_identity.as_str())
        .map_err(|_| crate::Status::Invalid)?;
    let native = vt::SnapshotMetadata {
        engine_identity: claimed,
        wire_profile: metadata.codec.wire_profile,
        library_version: metadata.codec.library_version,
        snapshot_version: metadata.codec.snapshot_version,
        state: Some(vt::ProfileState {
            abi_version: metadata.state.abi_version,
            terminal_profile: vt::TerminalProfile::Native,
            preceding_join: metadata.state.preceding_join,
        }),
        configuration: vt::RestorationConfig {
            runtime_image_storage_limit: metadata.configuration.runtime_image_storage_limit,
            runtime_resize_pull_scrollback: metadata.configuration.runtime_resize_pull_scrollback,
        },
    };
    let bytes = recovery.bytes().map_err(|_| crate::Status::Invalid)?;
    let compatibility = vt::Native0RawV1Compatibility::for_negotiated_sender(receiver, sender);
    let snapshot =
        vt::CrossTargetNative0RawV1Snapshot::from_untrusted(compatibility, &bytes, native, limits)
            .map_err(|_| crate::Status::Invalid)?;
    let (mut terminal, _report) = snapshot
        .decode(receiver, limits)
        .map_err(|_| crate::Status::Invalid)?;
    let screen = terminal.render().map_err(|_| crate::Status::Invalid)?;
    if (screen.cols, screen.rows) != (recovery.cols(), recovery.rows()) {
        return Err(crate::Status::Invalid);
    }
    Ok(terminal)
}

pub(crate) fn reset_ticket(mobile: &Mobile, terminal: u64) -> Result<u64> {
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if s.closed {
        return Err(STALE);
    }
    if !b.reset_required {
        return Err(EMPTY);
    }
    Ok(b.reset_ticket)
}
pub(crate) fn reset_grid(mobile: &Mobile, terminal: u64, ticket: u64) -> Result<(u16, u16)> {
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if s.closed || !b.reset_required || b.reset_ticket != ticket {
        return Err(STALE);
    }
    b.required_grid.ok_or(EMPTY)
}
pub(crate) fn reset_done(mobile: &Mobile, terminal: u64, ticket: u64) -> Result<()> {
    let connection = mobile.legacy()?.state().connection_id;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values_mut()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if !b.reset_required || b.reset_ticket != ticket {
        return Err(STALE);
    }
    let shared = crate::engine::lookup(terminal).map_err(|e| e as i32)?;
    if shared.owner != std::thread::current().id() {
        return Err(crate::Status::WrongThread as i32);
    }
    let q = shared.queue.lock().map_err(|_| PANIC)?;
    if q.failed || q.closed || q.accepted != 0 || q.consumed != 0 || !q.chunks.is_empty() {
        return Err(BUSY);
    }
    drop(q);
    if let Some((cols, rows)) = b.required_grid {
        let grid = crate::engine::with_engine(terminal, |owned| {
            let screen = owned.terminal.render().map_err(|_| crate::Status::Engine)?;
            Ok((screen.cols, screen.rows))
        })
        .map_err(|status| status as i32)?;
        if grid != (cols, rows) {
            return Err(INVALID);
        }
    }
    if let Some(value) = b.held.clone() {
        install(b, &value, connection)?;
        b.held = None;
    }
    b.reset_required = false;
    Ok(())
}

pub(crate) fn ready(binding: &Binding) -> Result<bool> {
    if binding.reset_required || binding.cursor.is_none() || binding.attaching.is_some() {
        return Ok(false);
    }
    if !binding.recovering {
        return Ok(true);
    }
    let Some(ticket) = binding.recovery_ticket else {
        return Ok(false);
    };
    let shared = crate::engine::lookup(binding.terminal).map_err(|e| e as i32)?;
    let queue = shared.queue.lock().map_err(|_| PANIC)?;
    Ok(!queue.failed && !queue.closed && queue.consumed >= ticket)
}
pub(crate) fn ready_handle(mobile: &Mobile, terminal: u64) -> Result<bool> {
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    let b = s
        .bindings
        .values()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    ready(b)
}
