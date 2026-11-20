use super::*;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    History,
    Settle,
    Delete,
    Shell,
    Accent,
    Background,
    Agents,
}
impl Field {
    pub fn from_patch(patch: &SettingsPatch) -> Self {
        if patch.history_page_size.is_some() {
            Self::History
        } else if patch.session_settle_hours.is_some() {
            Self::Settle
        } else if patch.session_delete_hours.is_some() {
            Self::Delete
        } else if patch.terminal_profile_id.is_some() {
            Self::Shell
        } else if patch.accent_color.is_some() {
            Self::Accent
        } else if patch.background_color.is_some() {
            Self::Background
        } else {
            Self::Agents
        }
    }
    fn input(self) -> Option<Input> {
        match self {
            Self::History => Some(Input::History),
            Self::Settle => Some(Input::Settle),
            Self::Delete => Some(Input::Delete),
            _ => None,
        }
    }
    fn category(self) -> Category {
        match self {
            Self::History | Self::Settle | Self::Delete | Self::Accent | Self::Background => {
                Category::General
            }
            Self::Shell => Category::Terminals,
            Self::Agents => Category::Agents,
        }
    }
}
impl SettingsPanel {
    pub(super) fn field_error(&self, field: Field) -> Option<String> {
        field
            .input()
            .and_then(|input| self.forms.errors.get(&input).cloned())
            .or_else(|| {
                self.save_error
                    .as_ref()
                    .filter(|(failed, _)| *failed == field)
                    .map(|(_, message)| message.clone())
            })
    }
    pub(super) fn category_error(&self, category: Category) -> bool {
        self.save_error
            .as_ref()
            .is_some_and(|(field, _)| field.category() == category)
            || match category {
                Category::General => [Input::History, Input::Settle, Input::Delete]
                    .iter()
                    .any(|field| self.forms.errors.contains_key(field)),
                _ => false,
            }
    }
}
