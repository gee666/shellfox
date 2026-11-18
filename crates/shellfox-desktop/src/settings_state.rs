use super::rule_edits::Edits;
use shellfox_core::{Result, Settings, SettingsPatch};
pub(super) struct Draft {
    pub server: Settings,
    pub value: Settings,
    pub pending: SettingsPatch,
    pub inflight: Option<SettingsPatch>,
    pub revision: u64,
    pub error: Option<String>,
    pending_rules: Edits,
    inflight_rules: Edits,
    inflight_base: Option<Settings>,
    inflight_revision: u64,
    failed_revision: Option<u64>,
}
impl Draft {
    pub fn new(settings: Settings) -> Result<Self> {
        settings.validate()?;
        Ok(Self {
            server: settings.clone(),
            value: settings,
            pending: SettingsPatch::default(),
            inflight: None,
            revision: 0,
            error: None,
            pending_rules: Edits::default(),
            inflight_rules: Edits::default(),
            inflight_base: None,
            inflight_revision: 0,
            failed_revision: None,
        })
    }
    pub fn stage(&mut self, mut patch: SettingsPatch) -> Result<bool> {
        let next = self.value.patched(patch.clone())?;
        if next == self.value {
            return Ok(false);
        }
        self.error = None;
        self.failed_revision = None;
        if let Some(rules) = patch.process_rules.take() {
            self.pending_rules
                .merge(Edits::between(&self.value.process_rules, &rules));
        }
        self.value = next;
        merge(&mut self.pending, patch);
        self.revision += 1;
        Ok(true)
    }
    pub fn dirty(&self) -> bool {
        self.inflight.is_some()
            || self.pending != SettingsPatch::default()
            || !self.pending_rules.empty()
    }
    pub fn queued_write(&self) -> bool {
        self.failed_revision != Some(self.revision)
            && (self.pending != SettingsPatch::default() || !self.pending_rules.empty())
    }
    pub fn take_save(&mut self) -> Option<SettingsPatch> {
        if self.inflight.is_some()
            || self.failed_revision == Some(self.revision)
            || (self.pending == SettingsPatch::default() && self.pending_rules.empty())
        {
            return None;
        }
        let mut patch = self.pending.clone();
        if !self.pending_rules.empty() {
            patch.process_rules = Some(self.pending_rules.apply(&self.server.process_rules));
        }
        if let Err(error) = self.server.patched(patch.clone()) {
            self.error = Some(error.to_string());
            self.failed_revision = Some(self.revision);
            return None;
        }
        self.pending = SettingsPatch::default();
        self.inflight_rules = std::mem::take(&mut self.pending_rules);
        self.inflight_base = Some(self.server.clone());
        self.inflight_revision = self.revision;
        self.inflight = Some(patch.clone());
        self.error = None;
        Some(patch)
    }
    pub fn observed(&mut self, settings: Settings) -> Result<()> {
        settings.validate()?;
        let mut next = settings.clone();
        if let Some(patch) = &self.inflight {
            next = apply(&next, patch, &self.inflight_rules)?;
        }
        next = apply(&next, &self.pending, &self.pending_rules)?;
        self.server = settings;
        self.value = next;
        Ok(())
    }
    pub fn saved(&mut self, mut settings: Settings) -> Result<()> {
        settings.validate()?;
        if let (Some(base), Some(sent)) = (&self.inflight_base, &self.inflight) {
            preserve_external(&mut settings, base, &self.server, sent);
            let mut external = Edits::between(&base.process_rules, &self.server.process_rules);
            external.exclude(&self.inflight_rules);
            settings.process_rules = external.apply(&settings.process_rules);
        }
        settings.validate()?;
        let next = apply(&settings, &self.pending, &self.pending_rules)?;
        self.inflight = None;
        self.inflight_rules = Edits::default();
        self.inflight_base = None;
        self.server = settings;
        self.value = next;
        self.error = None;
        self.failed_revision = None;
        Ok(())
    }
    pub fn failed(&mut self, message: String) {
        if let Some(mut failed) = self.inflight.take() {
            failed.process_rules = None;
            merge(&mut failed, std::mem::take(&mut self.pending));
            self.pending = failed;
            let mut rules = std::mem::take(&mut self.inflight_rules);
            rules.merge(std::mem::take(&mut self.pending_rules));
            self.pending_rules = rules;
        }
        self.inflight_base = None;
        self.failed_revision = Some(self.inflight_revision);
        self.error = Some(message);
    }
}
fn apply(base: &Settings, patch: &SettingsPatch, rules: &Edits) -> Result<Settings> {
    let mut patch = patch.clone();
    patch.process_rules = None;
    let mut next = base.patched(patch)?;
    next.process_rules = rules.apply(&next.process_rules);
    next.validate()?;
    Ok(next)
}
fn merge(target: &mut SettingsPatch, patch: SettingsPatch) {
    macro_rules! update{($($field:ident),*)=>{$(if patch.$field.is_some(){target.$field=patch.$field;})*};}
    update!(
        version,
        python_path,
        accent_color,
        background_color,
        adapter_id,
        shell_id,
        shell_executable,
        terminal_profile_id,
        history_page_size,
        session_settle_hours,
        session_delete_hours,
        auto_update,
        terminal_env
    );
}
fn preserve_external(
    target: &mut Settings,
    base: &Settings,
    current: &Settings,
    sent: &SettingsPatch,
) {
    macro_rules! keep{($($field:ident),*)=>{$(if sent.$field.is_none()&&base.$field!=current.$field{target.$field.clone_from(&current.$field);})*};}
    keep!(
        version,
        python_path,
        accent_color,
        background_color,
        adapter_id,
        terminal_profile_id,
        history_page_size,
        session_settle_hours,
        session_delete_hours,
        auto_update,
        terminal_env
    );
    if sent.terminal_profile_id.is_none() {
        keep!(shell_id, shell_executable);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_work_distinguishes_debounce_flight_and_retained_failed_draft() {
        let mut draft = Draft::new(Settings::default()).unwrap();
        assert!(!draft.queued_write());
        assert!(draft.inflight.is_none());
        draft
            .stage(SettingsPatch {
                auto_update: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert!(draft.queued_write());
        draft.take_save().unwrap();
        assert!(!draft.queued_write());
        assert!(draft.inflight.is_some());
        draft.failed("Unavailable".into());
        assert!(draft.dirty());
        assert!(!draft.queued_write());
        assert!(draft.inflight.is_none());
        draft
            .stage(SettingsPatch {
                history_page_size: Some(12),
                ..Default::default()
            })
            .unwrap();
        assert!(draft.queued_write());
    }
    #[test]
    fn newer_edits_survive_save_and_environment_is_omitted() {
        let mut d = Draft::new(Settings::default()).unwrap();
        d.stage(SettingsPatch {
            accent_color: Some("#123456".into()),
            ..Default::default()
        })
        .unwrap();
        let sent = d.take_save().unwrap();
        assert!(sent.terminal_env.is_none());
        d.stage(SettingsPatch {
            accent_color: Some("#abcdef".into()),
            ..Default::default()
        })
        .unwrap();
        assert!(d.take_save().is_none());
        d.saved(d.server.patched(sent).unwrap()).unwrap();
        assert_eq!(d.value.accent_color, "#abcdef");
        assert_eq!(
            d.take_save().unwrap().accent_color.as_deref(),
            Some("#abcdef")
        );
    }
    #[test]
    fn failure_does_not_retry_same_intent_but_allows_genuinely_newer_edits() {
        let mut d = Draft::new(Settings::default()).unwrap();
        d.stage(SettingsPatch {
            auto_update: Some(false),
            ..Default::default()
        })
        .unwrap();
        d.take_save().unwrap();
        d.failed("Unavailable".into());
        assert!(d.take_save().is_none());
        d.stage(SettingsPatch {
            history_page_size: Some(22),
            ..Default::default()
        })
        .unwrap();
        d.take_save().unwrap();
        d.stage(SettingsPatch {
            history_page_size: Some(23),
            ..Default::default()
        })
        .unwrap();
        d.failed("Unavailable".into());
        assert_eq!(d.take_save().unwrap().history_page_size, Some(23));
    }
    #[test]
    fn external_observations_and_rule_path_changes_survive_local_toggles() {
        let mut d = Draft::new(Settings::default()).unwrap();
        let mut rules = d.value.process_rules.clone();
        rules[0].enabled = false;
        d.stage(SettingsPatch {
            process_rules: Some(rules),
            ..Default::default()
        })
        .unwrap();
        let mut external = d.server.clone();
        external.history_page_size = 30;
        external.process_rules[0].executable_paths = vec!["/opt/pi".into()];
        d.observed(external).unwrap();
        let sent = d.take_save().unwrap();
        let rule = &sent.process_rules.unwrap()[0];
        assert!(!rule.enabled);
        assert_eq!(rule.executable_paths, ["/opt/pi"]);
        assert!(sent.history_page_size.is_none());
    }
    #[test]
    fn external_unowned_fields_are_not_reverted_by_an_older_save_reply() {
        let mut d = Draft::new(Settings::default()).unwrap();
        d.stage(SettingsPatch {
            auto_update: Some(false),
            ..Default::default()
        })
        .unwrap();
        let sent = d.take_save().unwrap();
        let reply = d.server.patched(sent).unwrap();
        let mut external = d.server.clone();
        external.history_page_size = 44;
        d.observed(external).unwrap();
        d.saved(reply).unwrap();
        assert_eq!(d.value.history_page_size, 44);
        assert!(!d.value.auto_update);
    }
}
