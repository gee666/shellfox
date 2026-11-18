use crate::{validation, Result};
use serde::{Deserialize, Serialize};

/// A process matcher, never a launch command. These values grant no execution capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsProcessRule {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub executable_basenames: Vec<String>,
    pub executable_paths: Vec<String>,
    pub script_path_suffixes: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "process_names"
    )]
    pub process_names: Option<Vec<String>>,
}
fn process_names<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Option<Vec<String>>, D::Error> {
    Vec::<String>::deserialize(de).map(Some)
}
impl SettingsProcessRule {
    pub fn validate(&self) -> Result<()> {
        let uuid = uuid::Uuid::parse_str(&self.id).is_ok_and(|id| {
            let bytes = self.id.as_bytes();
            id.to_string().eq_ignore_ascii_case(&self.id)
                && (id.is_nil()
                    || id == uuid::Uuid::max()
                    || matches!(bytes[14], b'1'..=b'8')
                        && matches!(bytes[19], b'8' | b'9' | b'a' | b'A' | b'b' | b'B'))
        });
        let custom = self.id.strip_prefix("custom-").is_some_and(|rest| {
            rest.rsplit_once('-').is_some_and(|(name, suffix)| {
                name.as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                    && !suffix.is_empty()
                    && suffix
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
        });
        validation::ensure(
            uuid || self.id.len() <= 120 && custom,
            "Invalid process rule ID.",
        )?;
        crate::settings_validation::nonblank(&self.label, 200)?;
        validation::ensure(
            !self.executable_basenames.is_empty() || !self.executable_paths.is_empty(),
            "Process rules need an executable matcher.",
        )?;
        for values in [
            self.executable_basenames.as_slice(),
            self.process_names.as_deref().unwrap_or(&[]),
        ] {
            validation::ensure(values.len() <= 30, "Too many executable names.")?;
            for value in values {
                crate::settings_validation::nonblank(value, 260)?;
                validation::ensure(!value.contains(['\\', '/', ':']), "Use a process basename.")?;
            }
        }
        validation::ensure(
            self.executable_paths.len() <= 30 && self.script_path_suffixes.len() <= 30,
            "Too many process paths.",
        )?;
        for path in &self.executable_paths {
            super::settings_validation::path(path, 32760)?;
        }
        for suffix in &self.script_path_suffixes {
            crate::settings_validation::nonblank(suffix, 4096)?;
            validation::ensure(
                !suffix
                    .split(['/', '\\'])
                    .any(|part| part == "." || part == ".."),
                "Script suffixes cannot traverse directories.",
            )?;
        }
        Ok(())
    }
}
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).into()).collect()
}
/// Exact shipped Electron rules in src/main/defaults.ts. Keep IDs and order stable.
pub fn default_process_rules() -> Vec<SettingsProcessRule> {
    let variants = [
        (
            "5870",
            "Pi",
            "pi",
            vec![
                "@earendil-works/pi-coding-agent/dist/cli.js",
                "@mariozechner/pi-coding-agent/dist/cli.js",
                "@earendil-works/pi-coding-agent/dist/bundle/cli.js",
                "@mariozechner/pi-coding-agent/dist/bundle/cli.js",
            ],
        ),
        (
            "5871",
            "Claude",
            "claude",
            vec!["@anthropic-ai/claude-code/cli.js"],
        ),
        ("5872", "Codex", "codex", vec!["@openai/codex/bin/codex.js"]),
        (
            "5874",
            "OpenCode",
            "opencode",
            vec!["opencode-ai/bin/opencode"],
        ),
    ];
    variants
        .into_iter()
        .map(|(suffix, label, name, scripts)| SettingsProcessRule {
            id: format!("f919fb1a-fb03-4a93-8b9b-1cde465d{suffix}"),
            label: label.into(),
            enabled: true,
            executable_basenames: strings(&["node.exe", "node", "nodejs", "bun.exe", "bun"]),
            executable_paths: vec![],
            script_path_suffixes: strings(&scripts),
            process_names: Some(vec![name.into(), format!("{name}.exe")]),
        })
        .collect()
}
