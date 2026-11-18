use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Input {
    History,
    Settle,
    Delete,
    Python,
    Agent,
}
impl Input {
    pub fn label(self) -> &'static str {
        match self {
            Self::History => "Sessions per page",
            Self::Settle => "Settle after (hours)",
            Self::Delete => "Delete settled after (hours)",
            Self::Python => "Python path",
            Self::Agent => "Add agent",
        }
    }
}
pub(super) struct Forms {
    pub fields: BTreeMap<Input, Entity<Field>>,
    pub seen: BTreeMap<Input, String>,
    pub errors: BTreeMap<Input, String>,
}
impl Forms {
    pub fn new(cx: &mut App) -> Self {
        let fields: BTreeMap<_, _> = [
            Input::History,
            Input::Settle,
            Input::Delete,
            Input::Python,
            Input::Agent,
        ]
        .into_iter()
        .map(|key| {
            let field = Field::new("", cx);
            field.update(cx, |field, cx| {
                field.configure(
                    crate::field::FieldOptions {
                        placeholder: Some(
                            if key == Input::Agent {
                                "Process name or absolute executable path"
                            } else {
                                ""
                            }
                            .into(),
                        ),
                        text_align: Some(
                            if matches!(key, Input::History | Input::Settle | Input::Delete) {
                                gpui::TextAlign::Right
                            } else {
                                gpui::TextAlign::Left
                            },
                        ),
                        ..Default::default()
                    },
                    cx,
                )
            });
            (key, field)
        })
        .collect();
        Self {
            fields,
            seen: BTreeMap::new(),
            errors: BTreeMap::new(),
        }
    }
    pub fn sync(&mut self, settings: &Settings, cx: &mut App) {
        for (key, text) in [
            (Input::History, settings.history_page_size.to_string()),
            (Input::Settle, settings.session_settle_hours.to_string()),
            (Input::Delete, settings.session_delete_hours.to_string()),
            (
                Input::Python,
                settings.python_path.clone().unwrap_or_default(),
            ),
        ] {
            if self.errors.contains_key(&key) {
                continue;
            }
            let field = self.fields[&key].read(cx);
            if field.is_composing() {
                continue;
            }
            let current = field.text.clone();
            let equivalent = current == text
                || matches!(key, Input::History | Input::Settle | Input::Delete)
                    && current.parse::<f64>().ok() == text.parse::<f64>().ok();
            if equivalent {
                self.seen.insert(key, current);
                continue;
            }
            self.seen.insert(key, text.clone());
            self.fields[&key].update(cx, |field, cx| field.set_text(text, cx));
        }
    }
}
pub(super) struct Environment {
    pub text: Entity<Field>,
    pub error: Option<String>,
    pub pending: bool,
}
impl SettingsPanel {
    pub(super) fn changed_input(&mut self, key: Input, text: String, cx: &mut Context<Self>) {
        if self.operations.frozen() {
            return;
        }
        if self.forms.seen.get(&key) == Some(&text) {
            return;
        }
        self.forms.seen.insert(key, text.clone());
        self.forms.errors.remove(&key);
        if key == Input::Agent {
            self.agent_error = None;
            cx.notify();
            return;
        }
        if key == Input::Python
            && !text.trim().is_empty()
            && (!text.starts_with('/') || text.chars().any(|ch| ch <= '\u{1f}' || ch == '\u{7f}'))
        {
            self.forms.errors.insert(
                key,
                "Use an absolute Python path, or leave empty for auto-detect.".into(),
            );
            cx.notify();
            return;
        }
        let patch = match key {
            Input::History => text
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && (1.0..=100.0).contains(n) && n.fract() == 0.0)
                .map(|n| SettingsPatch {
                    history_page_size: Some(n as u32),
                    ..Default::default()
                }),
            Input::Settle | Input::Delete => text
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && (1.0..=87600.0).contains(n))
                .map(|n| {
                    if key == Input::Settle {
                        SettingsPatch {
                            session_settle_hours: Some(n),
                            ..Default::default()
                        }
                    } else {
                        SettingsPatch {
                            session_delete_hours: Some(n),
                            ..Default::default()
                        }
                    }
                }),
            Input::Python => Some(SettingsPatch {
                python_path: Some(if text.trim().is_empty() {
                    None
                } else {
                    Some(text.clone())
                }),
                ..Default::default()
            }),
            Input::Agent => None,
        };
        if let Some(patch) = patch {
            self.stage(patch, cx);
        } else {
            self.forms.errors.insert(
                key,
                if key == Input::History {
                    "Use a whole number from 1 to 100."
                } else {
                    "Use a number from 1 to 87600."
                }
                .into(),
            );
            cx.notify();
        }
    }
    pub(super) fn add_agent(&mut self, cx: &mut Context<Self>) {
        if self.operations.frozen() {
            return;
        }
        let Some(draft) = &self.draft else {
            return;
        };
        let text = self.forms.fields[&Input::Agent].read(cx).text.clone();
        match rules::create(&text, &draft.value.process_rules) {
            Ok(rule) => {
                let mut rules = draft.value.process_rules.clone();
                rules.push(rule);
                self.stage(
                    SettingsPatch {
                        process_rules: Some(rules),
                        ..Default::default()
                    },
                    cx,
                );
                self.forms.seen.insert(Input::Agent, String::new());
                self.forms.fields[&Input::Agent]
                    .update(cx, |field, cx| field.set_text(String::new(), cx));
                self.agent_error = None;
            }
            Err(error) => self.agent_error = Some(error.to_string()),
        }
        cx.notify();
    }
    pub(super) fn open_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.environment_writing || self.environment_write.is_some() {
            return;
        }
        if let Some(draft) = &self.draft {
            let text = environment_data::format(&draft.value.terminal_env);
            let field = Field::textarea(text, cx);
            field.update(cx, |field, cx| field.apply_palette(self.palette, cx));
            self.environment_subscription =
                Some(cx.observe_in(&field, window, |_, _, _, cx| cx.notify()));
            field.read(cx).focus(window);
            self.environment = Some(Environment {
                text: field,
                error: None,
                pending: false,
            });
            cx.notify();
        }
    }
    pub(super) fn save_environment(&mut self, cx: &mut Context<Self>) {
        if self.operations.frozen() {
            return;
        }
        let Some(environment) = &mut self.environment else {
            return;
        };
        if environment.pending {
            return;
        }
        let Some(catalog) = &self.catalog else {
            environment.error = Some("Terminal platform information is unavailable.".into());
            cx.notify();
            return;
        };
        let text = environment.text.read(cx).text.clone();
        let (env, issues) = environment_data::parse(&text, catalog.platform == "win32");
        if !issues.is_empty() {
            environment.error = Some(
                issues
                    .iter()
                    .map(|issue| format!("Line {}: {}", issue.line, issue.message))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            cx.notify();
            return;
        }
        environment.pending = true;
        environment.error = None;
        environment
            .text
            .update(cx, |field, cx| field.set_read_only(true, cx));
        self.environment_write = Some(environment_rpc::PendingEnvironment {
            identity: environment.text.entity_id(),
            env,
        });
        self.begin_save(cx);
        cx.notify();
    }
}
