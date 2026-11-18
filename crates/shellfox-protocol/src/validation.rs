use crate::ValidationError;
use std::collections::HashSet;

pub fn optional<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}

pub fn nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(d)
}

pub fn present_nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(d).map(Some)
}

pub fn ensure(ok: bool) -> Result<(), ValidationError> {
    if ok {
        Ok(())
    } else {
        Err(ValidationError)
    }
}
pub fn text(value: &str, max: usize) -> Result<(), ValidationError> {
    ensure(
        value.encode_utf16().count() <= max
            && !value.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}'),
    )
}
pub fn title(value: &str) -> Result<(), ValidationError> {
    text(value, 200)?;
    ensure(!value.trim().is_empty())
}
pub fn timestamp(value: &str) -> Result<(), ValidationError> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value).map_err(|_| ValidationError)?;
    ensure(value.ends_with('Z') && parsed.offset().local_minus_utc() == 0)
}
pub fn optional_time(value: &Option<String>) -> Result<(), ValidationError> {
    if let Some(value) = value {
        timestamp(value)?;
    }
    Ok(())
}
pub fn profile(value: &str) -> Result<(), ValidationError> {
    text(value, 200)?;
    ensure(!value.is_empty())
}
pub fn cwd(value: &str) -> Result<(), ValidationError> {
    text(value, 32767)?;
    let bytes = value.as_bytes();
    let unix = value.starts_with('/') && !value.starts_with("//");
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let wsl = regex::Regex::new(r"(?i)^\\\\(?:wsl\.localhost|wsl\$)\\[^\\/]+(?:\\.*)?$")
        .expect("constant regex")
        .is_match(value);
    ensure(wsl || (unix || drive) && !value.starts_with("\\\\") && !value.contains("://"))
}
pub fn count(value: u64) -> Result<(), ValidationError> {
    ensure(value <= 1_000_000)
}
pub fn integer(value: i64) -> Result<(), ValidationError> {
    ensure(value.unsigned_abs() <= crate::MAX_SAFE_INTEGER)
}
pub fn sequence(value: u64) -> Result<(), ValidationError> {
    ensure(value <= crate::MAX_SAFE_INTEGER)
}
pub fn epoch(value: u64) -> Result<(), ValidationError> {
    sequence(value)?;
    ensure(value > 0)
}
pub fn color(value: &str) -> Result<(), ValidationError> {
    ensure(
        value.len() == 7
            && value.starts_with('#')
            && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit),
    )
}
pub fn unique<T: Eq + std::hash::Hash>(values: &[T], max: usize) -> Result<(), ValidationError> {
    ensure(values.len() <= max && values.iter().collect::<HashSet<_>>().len() == values.len())
}
pub fn env(values: &[crate::EnvVar], stored: bool) -> Result<(), ValidationError> {
    ensure(values.len() <= 200)?;
    let mut names = HashSet::new();
    for item in values {
        let name = item.name.trim();
        ensure(
            !name.is_empty()
                && name.encode_utf16().count() <= 256
                && !name.contains(['=', '\0'])
                && names.insert(name),
        )?;
        if !stored {
            ensure(
                !name
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, ':' | '/')),
            )?;
        }
        ensure(item.value.encode_utf16().count() <= 32767 && !item.value.contains('\0'))?;
        if !stored {
            ensure(!item.value.contains(['\r', '\n']))?;
        }
    }
    Ok(())
}
