use super::*;
use crate::accessibility::{SemanticAction, SemanticId};
impl SettingsPanel {
    pub(super) fn bind_field(
        &mut self,
        field: Entity<Field>,
        name: &str,
        parent: Option<SemanticId>,
        cx: &mut Context<Self>,
    ) -> Entity<Field> {
        let Some(binding) = self.semantic.clone() else {
            return field;
        };
        let parent = parent
            .or_else(|| self.category_semantic_id())
            .unwrap_or(binding.root);
        let result = field.update(cx, |input, cx| {
            input.set_frame(binding.frame.clone(), parent, name, cx)
        });
        match result {
            Ok(id) => {
                self.semantic_fields.insert(id, field.clone());
            }
            Err(error) => binding.frame.reject(error),
        }
        field
    }
    pub(super) fn route_field_action(
        &mut self,
        event: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let field = self.semantic_fields.get(&event.target)?.clone();
        if !self.visible || self.operations.frozen() || !self.current_field(&field) {
            return Some(false);
        }
        let accepted = field.update(cx, |input, cx| {
            input.handle_semantic_action(event, window, cx)
        });
        if accepted {
            cx.notify();
        }
        Some(accepted)
    }
    fn current_field(&self, field: &Entity<Field>) -> bool {
        let id = field.entity_id();
        if let Some(environment) = &self.environment {
            return environment.text.entity_id() == id;
        }
        match self.category {
            Category::General => [Input::History, Input::Settle, Input::Delete]
                .iter()
                .any(|key| self.forms.fields[key].entity_id() == id),
            Category::Agents => self.forms.fields[&Input::Agent].entity_id() == id,
            Category::Terminals => false,
            Category::RemoteAccess => {
                let Some(state) = &self.remote.state else {
                    return false;
                };
                self.remote.hostname.entity_id() == id
                    || state.enabled
                        && state.status != api::NetworkStatus::Online
                        && (self.remote.advanced && self.remote.auth.entity_id() == id
                            || self.remote.login.phase == login::Phase::Error
                                && self.remote.login.url.is_some()
                                && self.remote.login_address.entity_id() == id)
            }
            Category::Ssh => {
                self.ssh.editor.as_ref().is_some_and(|editor| {
                    editor.fields().iter().any(|input| input.entity_id() == id)
                })
            }
        }
    }
}
