//! ECMAScript String.trim and /\s/u match the current Zod environment schema.
//! Rust's Unicode whitespace differs at U+0085 and U+FEFF.
use crate::{validation, EnvVar, Result};
use serde::Deserialize;
use std::collections::HashSet;
pub(crate) fn js_whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
pub(crate) fn trim_js(value: &str) -> &str {
    value.trim_matches(js_whitespace)
}
pub(crate) fn deserialize_name<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<String, D::Error> {
    String::deserialize(de).map(|name| trim_js(&name).to_owned())
}
/// Normalizes every typed write just as Zod does before checking duplicates.
pub fn normalize_env(mut env: Vec<EnvVar>, case_insensitive: bool) -> Result<Vec<EnvVar>> {
    for entry in &mut env {
        entry.name = trim_js(&entry.name).to_owned();
    }
    validate_env(&env, case_insensitive)?;
    Ok(env)
}
pub fn validate_env(env: &[EnvVar], case_insensitive: bool) -> Result<()> {
    validation::ensure(env.len() <= 200, "Too many environment variables.")?;
    let mut names = HashSet::new();
    for item in env {
        validation::ensure(
            !item.name.is_empty()
                && item.name.encode_utf16().count() <= 256
                && trim_js(&item.name) == item.name
                && !item
                    .name
                    .chars()
                    .any(|c| js_whitespace(c) || matches!(c, '=' | ':' | '/' | '\0')),
            "Invalid environment variable name.",
        )?;
        validation::ensure(
            item.value.encode_utf16().count() <= 32767 && !item.value.contains(['\r', '\n', '\0']),
            "Invalid environment variable value.",
        )?;
        let name = if case_insensitive {
            item.name.to_lowercase()
        } else {
            item.name.clone()
        };
        validation::ensure(names.insert(name), "Duplicate environment variable name.")?;
    }
    Ok(())
}
