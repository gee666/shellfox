//! ANSI recovery is a negotiated projection, never a binary Ghostty snapshot.
use crate::RuntimeError;
use shellfox_ghostty_vt::{Terminal, FORMAT_EXTRAS_ALL, MAX_FORMAT_BYTES};

pub(crate) const MAX_RECOVERY_BYTES: usize = 4 * 1024 * 1024;

// CAN cancels an unfinished receiver sequence before RIS clears old content,
// history, margins and modes. Do not complete an old OSC with ST: that could
// execute a clipboard request left in the receiver's parser.
const RESET: &[u8] = b"\x18\x1bc";
// Public vt-1.0.1 formatter extras, excluding process-controlled OSC 7 metadata.
// Cell hyperlinks remain untrusted metadata; the client must apply its normal
// link policy. Cursor hyperlinks are restored for subsequent live output.
const FORMATTER_EXTRAS: u32 = FORMAT_EXTRAS_ALL & !(1 << 4);

/// Builds FormattedActiveBufferV1 from public getters only, without replaying history
/// into any parser or draining the authoritative terminal's pending PTY replies.
/// The caller freezes output/resize ordering at one sequence, sends this payload
/// before later chunks, and requires a recovery ACK before resuming delivery.
///
/// This includes active-buffer history and the formatter's supported extras,
/// followed by exact unfinished UTF-8/VT continuation bytes. It is not a claim
/// that every terminal mode, inactive buffer or saved cursor was restored.
/// vt-1.0.1's formatter can itself overwrite the saved cursor when restoring
/// pending wrap. FullState must not be advertised using this function.
/// Receivers must be passive mirrors: no query replies or clipboard effects.
pub(crate) fn active_buffer_ansi(terminal: &mut Terminal) -> Result<Vec<u8>, RuntimeError> {
    bounded_active_buffer_ansi(terminal, MAX_RECOVERY_BYTES)
}

pub(crate) fn remote_v1_ansi(terminal: &mut Terminal) -> Result<Vec<u8>, RuntimeError> {
    // v1 has a 256 KiB text limit. Leave room for UTF-8 substitution and framing.
    const BUDGET: usize = 240 * 1024;
    const ENTER_ALT: &[u8] = b"\x1b[?1049h";
    let info = terminal.recovery_info().map_err(crate::commands::engine)?;
    let prefix = if info.alternate { ENTER_ALT } else { &[] };
    let continuation = terminal.continuation().map_err(RuntimeError::unavailable)?;
    let budget = BUDGET.saturating_sub(RESET.len() + prefix.len() + continuation.len());
    // Formatter extras place the cursor and restore the available DEC modes
    // (including cursor keys, cursor visibility, bracketed paste and mouse).
    let formatted = terminal
        .format_vt(FORMATTER_EXTRAS, budget)
        .or_else(|_| newest_rows_within(terminal, budget))
        .or_else(|_| {
            // Large scrollback need not make the live terminal inaccessible.
            let screen = terminal.render()?;
            let bytes =
                crate::ansi::visible_screen(&screen).map_err(|_| shellfox_ghostty_vt::Error(-6))?;
            if bytes.len() <= budget {
                Ok(bytes)
            } else {
                Err(shellfox_ghostty_vt::Error(-6))
            }
        })
        .map_err(|_| {
            RuntimeError::validation("Terminal snapshot exceeds the v1 recovery limit.")
        })?;
    let mut bytes =
        Vec::with_capacity(RESET.len() + prefix.len() + formatted.len() + continuation.len());
    bytes.extend_from_slice(RESET);
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(&formatted);
    bytes.extend_from_slice(&continuation);
    Ok(bytes)
}

/// Formats the active buffer keeping only the newest rows that fit `budget`.
/// The formatter writes a preamble (palette, tab stops, home), then rows joined
/// by CRLF that each start from a reset pen, then a trailer that places the
/// cursor and restores modes. Dropping whole leading rows therefore leaves a
/// valid stream: no escape sequence or wide character spans a row boundary.
/// The kept rows always include the whole visible screen.
fn newest_rows_within(
    terminal: &mut Terminal,
    budget: usize,
) -> shellfox_ghostty_vt::Result<Vec<u8>> {
    const TOO_LARGE: shellfox_ghostty_vt::Error = shellfox_ghostty_vt::Error(-6);
    // Restart the pen and close any hyperlink before the first kept row.
    const ROW_RESET: &[u8] = b"\x1b[0m\x1b]8;;\x1b\\";
    let full = terminal.format_vt(FORMATTER_EXTRAS, MAX_FORMAT_BYTES)?;
    let rows = terminal.format_vt(0, MAX_FORMAT_BYTES)?;
    let start = find(&full, &rows).ok_or(TOO_LARGE)?;
    let (preamble, trailer) = (&full[..start], &full[start + rows.len()..]);
    let room = budget
        .checked_sub(preamble.len() + ROW_RESET.len() + trailer.len())
        .ok_or(TOO_LARGE)?;
    let screen_rows = usize::from(terminal.render()?.rows);
    let row_starts: Vec<usize> = std::iter::once(0)
        .chain(
            rows.windows(2)
                .enumerate()
                .filter(|(_, pair)| *pair == b"\r\n")
                .map(|(i, _)| i + 2),
        )
        .collect();
    let newest = row_starts.len().saturating_sub(screen_rows);
    let cut = row_starts
        .iter()
        .take(newest + 1)
        .copied()
        .find(|&start| rows.len() - start <= room)
        .ok_or(TOO_LARGE)?;
    let mut bytes = Vec::with_capacity(budget);
    bytes.extend_from_slice(preamble);
    bytes.extend_from_slice(ROW_RESET);
    bytes.extend_from_slice(&rows[cut..]);
    bytes.extend_from_slice(trailer);
    Ok(bytes)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let probe = &needle[..needle.len().min(64)];
    (0..=haystack.len() - needle.len())
        .find(|&at| haystack[at..].starts_with(probe) && haystack[at..].starts_with(needle))
}

fn bounded_active_buffer_ansi(
    terminal: &mut Terminal,
    max_bytes: usize,
) -> Result<Vec<u8>, RuntimeError> {
    // Read continuation before formatting. If tracking was disabled or exceeded
    // its bound, refuse the whole payload, not a prefix that loses parser state.
    let continuation = terminal.continuation().map_err(RuntimeError::unavailable)?;
    let budget = max_bytes
        .checked_sub(RESET.len())
        .and_then(|remaining| remaining.checked_sub(continuation.len()))
        .ok_or_else(|| RuntimeError::validation("ANSI continuation exceeds recovery limit"))?;
    let formatted = terminal
        .format_vt(FORMATTER_EXTRAS, budget)
        .map_err(RuntimeError::unavailable)?;
    let len = RESET
        .len()
        .checked_add(formatted.len())
        .and_then(|len| len.checked_add(continuation.len()))
        .filter(|len| *len <= max_bytes)
        .ok_or_else(|| RuntimeError::validation("ANSI recovery exceeds 4 MiB"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(RuntimeError::unavailable)?;
    bytes.extend_from_slice(RESET);
    bytes.extend_from_slice(&formatted);
    // This must remain last. No SGR, title, cursor or mode escape may follow an
    // unfinished parser suffix before the next ordered live output chunk.
    bytes.extend_from_slice(&continuation);
    Ok(bytes)
}

#[cfg(test)]
#[path = "recovery_resize_tests.rs"]
mod resize_tests;
#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
