use crate::{Error, Result};

/// PuTTY semantics: last user@ wins, one unbracketed colon discards its suffix.
/// The separate port is never inferred from that suffix.
pub fn normalize_endpoint(host: &str, user: &str) -> Result<(String, String)> {
    if host.encode_utf16().count() > 1000 || user.encode_utf16().count() > 200 {
        return Err(Error::Limit);
    }
    let host = host.trim_start_matches([' ', '\t']);
    let (host, user) = host.rsplit_once('@').map_or((host, user), |(u, h)| (h, u));
    let mut depth = 0u32;
    let mut colons = Vec::new();
    for (i, ch) in host.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            ':' if depth == 0 => colons.push(i),
            _ => {}
        }
    }
    let host = if colons.len() == 1 {
        &host[..colons[0]]
    } else {
        host
    };
    let mut host = host.replace([' ', '\t'], "");
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let address = inner.split('%').next().unwrap_or_default();
        if address.matches(':').count() > 1
            && address.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
            && !inner.contains(']')
        {
            host = inner.to_owned();
        }
    }
    if host.is_empty() || host.chars().any(|c| c == '@' || js_whitespace(c)) {
        return Err(Error::Validation);
    }
    text(&host, 1000)?;
    text(user, 200)?;
    Ok((host, user.to_owned()))
}

pub(crate) fn js_whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}'..='\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

pub(crate) fn text(value: &str, max: usize) -> Result<()> {
    if value.encode_utf16().count() > max {
        return Err(Error::Limit);
    }
    if value.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}') {
        return Err(Error::Validation);
    }
    Ok(())
}
