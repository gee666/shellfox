//! `shellfox help [group]`, laid out as 0.9.15's src/cli/help.ts did.
use crate::human_text::{wrap, Style};

pub(super) struct Group {
    key: &'static str,
    heading: &'static str,
    rows: &'static [(&'static str, &'static str)],
    notes: &'static [&'static str],
}
#[path = "help_groups.rs"]
mod groups;
use groups::{ALIASES, GROUPS};

fn group(topic: &str) -> Option<&'static Group> {
    let first = topic.split(' ').next().unwrap_or_default();
    let key = ALIASES
        .iter()
        .find(|(alias, _)| *alias == first)
        .map_or(first, |(_, key)| *key);
    GROUPS.iter().find(|group| group.key == key)
}
pub fn known(topic: &str) -> bool {
    group(topic).is_some()
}

pub fn text(topic: Option<&str>, style: Style, columns: usize) -> String {
    let selected: Vec<&Group> = match topic {
        Some(topic) => group(topic).into_iter().collect(),
        None => GROUPS.iter().collect(),
    };
    let mut lines = vec![
        format!("{} {}", style.accent("shellfox"), env!("SHELLFOX_VERSION")),
        String::new(),
    ];
    for group in selected {
        lines.push(style.accent(group.heading));
        let width = group
            .rows
            .iter()
            .map(|(command, _)| command.len())
            .max()
            .unwrap_or(0);
        for (command, description) in group.rows {
            if width.saturating_add(description.len() + 4) <= columns {
                lines.push(format!("  {command:width$}  {description}"));
            } else {
                lines.push(format!("  {command}"));
                lines.push(format!("    {description}"));
            }
        }
        for note in group.notes {
            if topic.is_some() || note.starts_with("Alias:") || note.starts_with("Aliases:") {
                lines.push(format!("  {note}"));
            }
        }
        lines.push(String::new());
    }
    lines.extend(
        [
            "Global options",
            "  --host <local|peer-id|hostname>  Select the host",
            "  --user-data <path>              Select the host data directory",
            "  --json                         Structured output and errors; never prompt",
            "  --yes                          Confirm destructive operations",
            "",
            "Destructive noninteractive operations require --yes. NO_COLOR disables color.",
            "Run shellfox help <group> for context rules and options.",
            "",
        ]
        .map(str::to_owned),
    );
    wrap(&lines.join("\n"), columns)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_aliases_and_notes_follow_0_9() {
        let all = text(None, Style(false), usize::MAX);
        assert!(all.starts_with(&format!(
            "shellfox {}\n\nHost\n  host run  ",
            env!("SHELLFOX_VERSION")
        )));
        assert!(all.contains("  Alias: tm. There is no tab command.\n"));
        assert!(!all.contains("Attach requires a TTY"));
        assert!(all.ends_with("Run shellfox help <group> for context rules and options.\n"));
        let terminals = text(Some("tm"), Style(false), usize::MAX);
        assert!(terminals.contains("Attach requires a TTY") && !terminals.contains("\nHost\n"));
        assert!(known("api") && known("ss") && !known("nope"));
        let narrow = text(Some("ssh"), Style(false), 30);
        assert!(narrow.contains("  ssh [--profile name]\n    Connect to a saved SSH"));
    }
}
