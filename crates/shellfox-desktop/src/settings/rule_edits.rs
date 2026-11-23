use shellfox_core::SettingsProcessRule;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Default)]
struct Fields {
    label: Option<String>,
    enabled: Option<bool>,
    basenames: Option<Vec<String>>,
    paths: Option<Vec<String>>,
    suffixes: Option<Vec<String>>,
    names: Option<Option<Vec<String>>>,
}
impl Fields {
    fn between(a: &SettingsProcessRule, b: &SettingsProcessRule) -> Self {
        Self {
            label: (a.label != b.label).then(|| b.label.clone()),
            enabled: (a.enabled != b.enabled).then_some(b.enabled),
            basenames: (a.executable_basenames != b.executable_basenames)
                .then(|| b.executable_basenames.clone()),
            paths: (a.executable_paths != b.executable_paths).then(|| b.executable_paths.clone()),
            suffixes: (a.script_path_suffixes != b.script_path_suffixes)
                .then(|| b.script_path_suffixes.clone()),
            names: (a.process_names != b.process_names).then(|| b.process_names.clone()),
        }
    }
    fn empty(&self) -> bool {
        self.label.is_none()
            && self.enabled.is_none()
            && self.basenames.is_none()
            && self.paths.is_none()
            && self.suffixes.is_none()
            && self.names.is_none()
    }
    fn merge(&mut self, newer: Self) {
        macro_rules! update{($($field:ident),*)=>{$(if newer.$field.is_some(){self.$field=newer.$field;})*};}
        update!(label, enabled, basenames, paths, suffixes, names);
    }
    fn apply(&self, rule: &mut SettingsProcessRule) {
        if let Some(v) = &self.label {
            rule.label = v.clone();
        }
        if let Some(v) = self.enabled {
            rule.enabled = v;
        }
        if let Some(v) = &self.basenames {
            rule.executable_basenames = v.clone();
        }
        if let Some(v) = &self.paths {
            rule.executable_paths = v.clone();
        }
        if let Some(v) = &self.suffixes {
            rule.script_path_suffixes = v.clone();
        }
        if let Some(v) = &self.names {
            rule.process_names = v.clone();
        }
    }
    fn exclude(&mut self, owned: &Self) {
        macro_rules! clear{($($field:ident),*)=>{$(if owned.$field.is_some(){self.$field=None;})*};}
        clear!(label, enabled, basenames, paths, suffixes, names);
    }
}
/// Source SettingsController semantics: rule fields are edits, not a stale array.
#[derive(Clone, Default)]
pub(super) struct Edits {
    added: Vec<SettingsProcessRule>,
    removed: BTreeSet<String>,
    fields: BTreeMap<String, Fields>,
}
impl Edits {
    pub fn empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.fields.is_empty()
    }
    pub fn between(before: &[SettingsProcessRule], after: &[SettingsProcessRule]) -> Self {
        let mut result = Self::default();
        for old in before {
            match after.iter().find(|rule| rule.id == old.id) {
                Some(new) => {
                    let fields = Fields::between(old, new);
                    if !fields.empty() {
                        result.fields.insert(old.id.clone(), fields);
                    }
                }
                None => {
                    result.removed.insert(old.id.clone());
                }
            }
        }
        result.added = after
            .iter()
            .filter(|rule| !before.iter().any(|old| old.id == rule.id))
            .cloned()
            .collect();
        result
    }
    pub fn apply(&self, before: &[SettingsProcessRule]) -> Vec<SettingsProcessRule> {
        let mut rules = before
            .iter()
            .filter(|rule| {
                !self.removed.contains(&rule.id)
                    && !self.added.iter().any(|added| added.id == rule.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        rules.extend(self.added.clone());
        for rule in &mut rules {
            if let Some(fields) = self.fields.get(&rule.id) {
                fields.apply(rule);
            }
        }
        rules
    }
    pub fn merge(&mut self, newer: Self) {
        for id in newer.removed {
            self.added.retain(|rule| rule.id != id);
            self.fields.remove(&id);
            self.removed.insert(id);
        }
        for rule in newer.added {
            self.removed.remove(&rule.id);
            self.fields.remove(&rule.id);
            self.added.retain(|old| old.id != rule.id);
            self.added.push(rule);
        }
        for (id, fields) in newer.fields {
            self.fields.entry(id).or_default().merge(fields);
        }
    }
    pub fn exclude(&mut self, owned: &Self) {
        for id in &owned.removed {
            self.removed.remove(id);
            self.added.retain(|rule| &rule.id != id);
            self.fields.remove(id);
        }
        for rule in &owned.added {
            self.removed.remove(&rule.id);
            self.added.retain(|other| other.id != rule.id);
            self.fields.remove(&rule.id);
        }
        for (id, fields) in &owned.fields {
            if let Some(external) = self.fields.get_mut(id) {
                external.exclude(fields);
                if external.empty() {
                    self.fields.remove(id);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toggles_preserve_external_paths_and_new_rules() {
        let base = shellfox_core::default_process_rules();
        let mut local = base.clone();
        local[0].enabled = false;
        let edits = Edits::between(&base, &local);
        let mut external = base.clone();
        external[0].executable_paths = vec!["/opt/pi".into()];
        let mut extra = base[0].clone();
        extra.id = "custom-new-agent-id".into();
        external.push(extra);
        let applied = edits.apply(&external);
        assert!(!applied[0].enabled);
        assert_eq!(applied[0].executable_paths, ["/opt/pi"]);
        assert_eq!(applied.len(), external.len());
    }
}
