use shellfox_core::{Error, ErrorCode, Result, SessionId, SettingsProcessRule};

pub(super) fn builtin_label(rule: &SettingsProcessRule) -> Option<&'static str> {
    match rule.id.as_str() {
        "f919fb1a-fb03-4a93-8b9b-1cde465d5870" => Some("Pi"),
        "f919fb1a-fb03-4a93-8b9b-1cde465d5871" => Some("Claude"),
        "f919fb1a-fb03-4a93-8b9b-1cde465d5872" => Some("Codex"),
        "f919fb1a-fb03-4a93-8b9b-1cde465d5874" => Some("OpenCode"),
        _ => None,
    }
}
fn name_key(value: &str) -> String {
    let lower = value.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
}
fn path_key(value: &str) -> String {
    if value.as_bytes().get(1) == Some(&b':') {
        value.replace('\\', "/").to_lowercase()
    } else {
        value.into()
    }
}
pub(super) fn create(input: &str, existing: &[SettingsProcessRule]) -> Result<SettingsProcessRule> {
    let input = input.trim();
    let is_path =
        shellfox_core::validation::cwd(input).is_ok() && input.encode_utf16().count() <= 32760;
    if !is_path
        && (input.is_empty()
            || input.encode_utf16().count() > 200
            || name_key(input).is_empty()
            || input.chars().any(|ch| {
                ch.is_whitespace() || ch.is_control() || matches!(ch, '/' | '\\' | ':' | ',')
            }))
    {
        return Err(Error::new(
            ErrorCode::Validation,
            "Enter a process name or an absolute local executable path, not a command.",
        ));
    }
    if existing.len() >= 100 {
        return Err(Error::new(ErrorCode::Validation, "The agent list is full."));
    }
    let duplicate = existing.iter().any(|rule| {
        if is_path {
            rule.executable_paths
                .iter()
                .any(|path| path_key(path) == path_key(input))
        } else if !rule.executable_paths.is_empty() {
            false
        } else {
            rule.process_names
                .as_deref()
                .unwrap_or(if rule.script_path_suffixes.is_empty() {
                    &rule.executable_basenames
                } else {
                    &[]
                })
                .iter()
                .any(|name| name_key(name) == name_key(input))
        }
    });
    if duplicate {
        return Err(Error::new(
            ErrorCode::Validation,
            "That program is already listed.",
        ));
    }
    let name = if is_path {
        input.rsplit(['/', '\\']).next().unwrap_or(input)
    } else {
        if input.to_lowercase().ends_with(".exe") {
            &input[..input.len() - 4]
        } else {
            input
        }
    };
    let mut slug = String::new();
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.len() == 60 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    let id = format!(
        "custom-{}-{}",
        if slug.is_empty() { "agent" } else { slug },
        SessionId::new()
    );
    let rule = SettingsProcessRule {
        id,
        label: if is_path { name } else { input }
            .chars()
            .take(200)
            .collect(),
        enabled: true,
        executable_basenames: if is_path {
            vec![]
        } else {
            vec![
                name.into(),
                if input.to_lowercase().ends_with(".exe") {
                    input.into()
                } else {
                    format!("{name}.exe")
                },
            ]
        },
        executable_paths: if is_path { vec![input.into()] } else { vec![] },
        script_path_suffixes: vec![],
        process_names: None,
    };
    rule.validate()?;
    Ok(rule)
}
pub(super) fn summary(rule: &SettingsProcessRule) -> String {
    let mut parts = Vec::new();
    if !rule.executable_basenames.is_empty() {
        parts.push(format!(
            "Process name: {}",
            rule.executable_basenames.join(", ")
        ));
    }
    parts.extend(
        rule.executable_paths
            .iter()
            .map(|path| format!("Exact path: {path}")),
    );
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_paths_and_duplicates_match_electron_rules() {
        let rule = create("my-agent", &[]).unwrap();
        assert_eq!(rule.executable_basenames, ["my-agent", "my-agent.exe"]);
        assert!(create("MY-AGENT.exe", &[rule]).is_err());
        let rule = create("C:\\Tools\\My Agent.exe", &[]).unwrap();
        assert_eq!(rule.executable_paths, ["C:\\Tools\\My Agent.exe"]);
        assert!(create("c:/tools/my agent.exe", &[rule]).is_err());
        for bad in ["", "node --flag", "tool,other", "./tool", "C:tool"] {
            assert!(create(bad, &[]).is_err());
        }
    }
}
