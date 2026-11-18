use crate::{Error, ErrorCode, Result};
pub fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::new(ErrorCode::Validation, message))
    }
}
pub fn text(value: &str, max: usize) -> Result<()> {
    ensure(
        !value.trim().is_empty()
            && value.encode_utf16().count() <= max
            && !value.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}'),
        "Invalid text.",
    )
}
pub fn title(value: &str) -> Result<()> {
    text(value, 200)
}
pub fn cwd(value: &str) -> Result<()> {
    text(value, 32767)?;
    let bytes = value.as_bytes();
    let unix = value.starts_with('/') && !value.starts_with("//");
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let lower = value.to_ascii_lowercase();
    let wsl = ["\\\\wsl$\\", "\\\\wsl.localhost\\"].iter().any(|prefix| {
        lower.strip_prefix(prefix).is_some_and(|rest| {
            rest.split('\\')
                .next()
                .is_some_and(|distro| !distro.is_empty() && !distro.contains('/'))
        })
    });
    ensure(
        (unix || drive || wsl) && !value.contains("://"),
        "Expected an absolute local directory.",
    )
}
pub fn color(value: &str) -> Result<()> {
    ensure(
        value.len() == 7
            && value.starts_with('#')
            && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit),
        "Expected a six-digit hex color.",
    )
}
pub fn timestamp(value: &str) -> Result<()> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| Error::new(ErrorCode::Validation, "Expected a UTC timestamp."))?;
    ensure(
        value.ends_with('Z') && parsed.offset().local_minus_utc() == 0,
        "Expected a UTC timestamp.",
    )
}
