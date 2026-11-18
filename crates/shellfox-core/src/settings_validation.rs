use crate::{validation::ensure, EnvVar, Result};
use serde::Deserialize;

pub(crate) fn environment<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<EnvVar>, D::Error> {
    Vec::<EnvVar>::deserialize(deserializer)
}
pub(crate) fn environment_patch<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Vec<EnvVar>>, D::Error> {
    environment(deserializer).map(Some)
}

// JSON 10 and 10.0 are the same JS number and both pass z.number().int().
fn integer<'de, D: serde::Deserializer<'de>>(
    de: D,
    max: f64,
) -> std::result::Result<f64, D::Error> {
    let value = f64::deserialize(de)?;
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=max).contains(&value) {
        return Err(serde::de::Error::custom("Expected an unsigned integer."));
    }
    Ok(value)
}
pub(crate) fn version<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<u8, D::Error> {
    integer(de, u8::MAX.into()).map(|n| n as u8)
}
pub(crate) fn history<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<u32, D::Error> {
    integer(de, u32::MAX.into()).map(|n| n as u32)
}
pub(crate) fn version_patch<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<u8>, D::Error> {
    version(de).map(Some)
}
pub(crate) fn history_patch<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<u32>, D::Error> {
    history(de).map(Some)
}

pub(crate) fn profile(value: &str) -> Result<()> {
    text(value, 200)?;
    ensure(!value.is_empty(), "Invalid terminal profile ID.")
}
pub(crate) fn nonblank(value: &str, max: usize) -> Result<()> {
    text(value, max)?;
    ensure(
        !crate::settings_env::trim_js(value).is_empty(),
        "Invalid settings text.",
    )
}
fn text(value: &str, max: usize) -> Result<()> {
    ensure(
        value.encode_utf16().count() <= max
            && !value.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}'),
        "Invalid settings text.",
    )
}
pub(crate) fn path(value: &str, max: usize) -> Result<()> {
    text(value, max)?;
    let bytes = value.as_bytes();
    let unix = value.starts_with('/') && !value.starts_with("//");
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let lower = value.to_ascii_lowercase();
    let wsl = ["\\\\wsl$\\", "\\\\wsl.localhost\\"].iter().any(|prefix| {
        lower.strip_prefix(prefix).is_some_and(|rest| {
            let distro = rest.split('\\').next().unwrap_or("");
            !distro.is_empty() && !distro.contains('/')
        })
    });
    ensure(
        wsl || (unix || drive) && !value.starts_with("\\\\") && !value.contains("://"),
        "Expected an absolute local path.",
    )
}
