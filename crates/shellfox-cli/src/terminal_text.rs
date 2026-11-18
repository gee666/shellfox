//! Metadata display rules from src/cli/terminal-text.ts, preserving valid JSON.

/// Serde already escapes C0 inside JSON strings. Escape the remaining terminal
/// controls and bidi formatting characters without escaping layout newlines.
pub fn escape_json_display(json: &str) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut output = String::with_capacity(json.len());
    for c in json.chars() {
        if matches!(c, '\u{7f}'..='\u{9f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            output.push_str("\\u");
            for shift in [12, 8, 4, 0] {
                output.push(char::from(HEX[((c as u32 >> shift) & 15) as usize]));
            }
        } else {
            output.push(c);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_metadata_is_visible_without_changing_json_values() {
        let value = serde_json::json!({"title": "\x1b[6n\u{9b}6n\u{202e}title\u{2066}\u{7f}"});
        let escaped = escape_json_display(&serde_json::to_string_pretty(&value).unwrap());
        assert!(!escaped.contains(['\x1b', '\u{9b}', '\u{202e}', '\u{2066}', '\u{7f}']));
        assert!(escaped.contains("\\u009b") && escaped.contains("\\u202e"));
        assert!(escaped.contains('\n'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&escaped).unwrap(),
            value
        );
    }
    #[test]
    fn normal_unicode_and_json_escapes_are_unchanged() {
        let json =
            serde_json::to_string_pretty(&serde_json::json!({"title": "雪🙂\\u009b"})).unwrap();
        assert_eq!(escape_json_display(&json), json);
    }
}
