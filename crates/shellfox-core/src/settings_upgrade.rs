//! Preserve user edits while upgrading only recognizable shipped process rules.
use crate::{default_process_rules, Settings, SettingsProcessRule};
use std::collections::HashSet;
const NATIVE: &str = "f919fb1a-fb03-4a93-8b9b-1cde465d5873";
fn same(a: &[String], b: &[String]) -> bool {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    b.sort();
    a == b
}
fn variants(names: &[&str]) -> Vec<Vec<String>> {
    vec![
        names.iter().map(|n| (*n).into()).collect(),
        names.iter().map(|n| format!("{n}.exe")).collect(),
        names
            .iter()
            .flat_map(|n| [(*n).into(), format!("{n}.exe")])
            .collect(),
    ]
}
impl Settings {
    /// Same migration as src/main/defaults.ts::upgradeBundledRules. Custom IDs,
    /// toggles and exact paths survive. Nothing here can execute a program.
    pub fn upgrade_bundled_rules(&mut self) {
        let bundled = default_process_rules();
        let native = self.process_rules.iter().find(|r| r.id == NATIVE).cloned();
        let native_defaults = native.as_ref().is_some_and(|r| {
            r.label == "Native agents"
                && r.executable_paths.is_empty()
                && r.script_path_suffixes.is_empty()
                && r.process_names.is_none()
                && [
                    variants(&["claude", "codex", "opencode"]),
                    variants(&["pi", "claude", "codex", "opencode"]),
                ]
                .concat()
                .iter()
                .any(|names| same(&r.executable_basenames, names))
        });
        let missing: Vec<_> = bundled
            .iter()
            .filter(|r| {
                native_defaults
                    && !self.process_rules.iter().any(|old| old.id == r.id)
                    && r.process_names.as_ref().is_some_and(|names| {
                        names.iter().any(|name| {
                            native
                                .as_ref()
                                .is_some_and(|n| n.executable_basenames.contains(name))
                        })
                    })
            })
            .cloned()
            .collect();
        let consolidate = native_defaults && self.process_rules.len() - 1 + missing.len() <= 100;
        let mut ids: HashSet<_> = self.process_rules.iter().map(|r| r.id.clone()).collect();
        let mut rules = vec![];
        for rule in &self.process_rules {
            if rule.id == NATIVE {
                if !consolidate {
                    rules.push(as_custom(rule, "native-agents", "Native agents", &mut ids));
                }
                continue;
            }
            let Some(current) = bundled.iter().find(|r| r.id == rule.id) else {
                rules.push(rule.clone());
                continue;
            };
            let old_pi = vec![
                "@earendil-works/pi-coding-agent/dist/cli.js".into(),
                "@mariozechner/pi-coding-agent/dist/cli.js".into(),
            ];
            let legacy_label = format!("{} Node launcher", current.label);
            let shipped = [current.label.as_str(), legacy_label.as_str()]
                .contains(&rule.label.as_str())
                && rule.executable_paths.is_empty()
                && rule.process_names.as_ref().is_none_or(|names| {
                    current
                        .process_names
                        .as_ref()
                        .is_some_and(|b| same(names, b))
                })
                && (variants(&["node"])
                    .iter()
                    .any(|names| same(&rule.executable_basenames, names))
                    || same(&rule.executable_basenames, &current.executable_basenames))
                && (same(&rule.script_path_suffixes, &current.script_path_suffixes)
                    || current.label == "Pi" && same(&rule.script_path_suffixes, &old_pi));
            if shipped {
                let mut updated = current.clone();
                updated.enabled = rule.enabled;
                rules.push(updated);
            } else {
                rules.push(as_custom(rule, &current.label, &legacy_label, &mut ids));
            }
        }
        if consolidate {
            for mut rule in missing {
                rule.enabled = native.as_ref().is_some_and(|r| r.enabled);
                rules.push(rule);
            }
        }
        self.process_rules = rules;
    }
}
fn as_custom(
    rule: &SettingsProcessRule,
    key: &str,
    bundled_label: &str,
    ids: &mut HashSet<String>,
) -> SettingsProcessRule {
    let base = format!("custom-bundled-{}-legacy", key.to_lowercase());
    let mut id = base.clone();
    let mut n = 2;
    while ids.contains(&id) {
        id = format!("{base}{n}");
        n += 1;
    }
    ids.insert(id.clone());
    SettingsProcessRule {
        id,
        label: if rule.label == bundled_label {
            format!("Custom {key}")
        } else {
            rule.label.clone()
        },
        ..rule.clone()
    }
}
