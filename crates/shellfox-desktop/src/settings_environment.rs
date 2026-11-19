use shellfox_core::EnvVar;
use std::collections::HashSet;
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Issue {
    pub line: usize,
    pub message: &'static str,
}
pub(crate) fn parse(text: &str, case_insensitive: bool) -> (Vec<EnvVar>, Vec<Issue>) {
    let mut env = Vec::new();
    let mut issues = Vec::new();
    let mut names = HashSet::new();
    let mut entries = 0;
    for (index, segment) in text.split_inclusive('\n').enumerate() {
        let raw = if let Some(raw) = segment.strip_suffix('\n') {
            raw.strip_suffix('\r').unwrap_or(raw)
        } else {
            segment
        };
        let line = index + 1;
        let mut issue = |message| issues.push(Issue { line, message });
        if raw.contains('\r') {
            issue("Values must be single-line with no CR or LF.");
            continue;
        }
        let source = raw.trim();
        if source.is_empty() || source.starts_with('#') {
            continue;
        }
        let entry = source
            .strip_prefix("export")
            .filter(|rest| rest.starts_with([' ', '\t']))
            .map(str::trim_start)
            .unwrap_or(source);
        let Some((name, value)) = entry.split_once('=') else {
            issue("Use NAME=value.");
            continue;
        };
        entries += 1;
        if entries > 200 {
            issue("Use at most 200 variables.");
            continue;
        }
        let name = name.trim();
        let mut value = value.trim();
        if name.is_empty()
            || name.encode_utf16().count() > 256
            || name
                .chars()
                .any(|ch| ch.is_whitespace() || matches!(ch, '=' | ':' | '/' | '\0'))
        {
            issue("Name must be 1 to 256 characters with no whitespace, =, :, / or NUL.");
            continue;
        }
        if value.starts_with(['\'', '"']) {
            let quote = value.as_bytes()[0];
            if value.len() < 2 || value.as_bytes().last() != Some(&quote) {
                issue("Values must be single-line; close the quoted value.");
                continue;
            }
            value = &value[1..value.len() - 1];
        }
        if value.encode_utf16().count() > 32767 || value.contains(['\r', '\n', '\0']) {
            issue("Value must be at most 32767 characters with no CR, LF or NUL.");
            continue;
        }
        let key = if case_insensitive {
            name.to_lowercase()
        } else {
            name.to_owned()
        };
        if !names.insert(key) {
            issue("Duplicate variable name.");
            continue;
        }
        env.push(EnvVar {
            name: name.into(),
            value: value.into(),
        });
    }
    (env, issues)
}
pub(crate) fn format(env: &[EnvVar]) -> String {
    env.iter()
        .map(|entry| {
            format!(
                "{}={}",
                entry.name,
                if entry.value.trim() != entry.value
                    || entry.value.starts_with(['\'', '"'])
                    || entry.value.contains(['\r', '\n'])
                {
                    format!("\"{}\"", entry.value)
                } else {
                    entry.value.clone()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_comments_export_and_equals_match_source() {
        let (env, issues) = parse("# local\nexport\tA=\"with=equals\"\nB='value'\n", false);
        assert!(issues.is_empty());
        assert_eq!(env[0].value, "with=equals");
        assert_eq!(env[1].value, "value");
    }
    #[test]
    fn errors_remain_line_numbered_and_do_not_hide_later_issues() {
        let (_, issues) = parse("PATH=one\npath=two\ninvalid\nA='unclosed", true);
        assert_eq!(
            issues.iter().map(|issue| issue.line).collect::<Vec<_>>(),
            [2, 3, 4]
        );
        assert_eq!(issues[0].message, "Duplicate variable name.");
        assert!(parse("PATH=one\npath=two", false).1.is_empty());
    }
    #[test]
    fn format_preserves_whitespace_and_literal_quote_values() {
        let values = vec![
            EnvVar {
                name: "A".into(),
                value: " padded ".into(),
            },
            EnvVar {
                name: "B".into(),
                value: "'literal".into(),
            },
        ];
        assert_eq!(parse(&format(&values), false).0, values);
    }
}
