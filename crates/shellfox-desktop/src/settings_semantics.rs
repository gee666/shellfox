use super::*;
use crate::accessibility::{
    Role, SemanticAction, SemanticActionKind, SemanticError, SemanticFrame, SemanticId,
    SemanticNode,
};
use std::{cell::Cell, rc::Rc};
#[derive(Clone)]
pub(super) struct Binding {
    pub frame: SemanticFrame,
    pub parent: SemanticId,
    pub root: SemanticId,
}
pub(super) struct Invocation {
    pub key: String,
    pub action: controls::Action,
    pub enabled: Rc<Cell<bool>>,
    pub focus: FocusHandle,
}
impl SettingsPanel {
    /// Root supplies its ONE window-lifetime collector. Its outer finish/cache
    /// boundaries own begin/publish/replay; a panel never starts another frame.
    pub fn set_frame(
        &mut self,
        frame: SemanticFrame,
        parent: SemanticId,
        cx: &mut Context<Self>,
    ) -> Result<SemanticId, SemanticError> {
        let root = frame.id_for("settings/panel")?;
        if let Some(current) = &self.semantic {
            if !current.frame.same_context(&frame) {
                return Err(SemanticError::FrameWindow);
            }
            if current.parent == parent {
                return Ok(current.root);
            }
        }
        self.semantic = Some(Binding {
            frame,
            parent,
            root,
        });
        cx.notify();
        Ok(root)
    }
    pub fn semantic_root_id(&self) -> Option<SemanticId> {
        self.semantic.as_ref().map(|binding| binding.root)
    }
    pub fn handle_semantic_action(
        &mut self,
        event: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.visible || self.operations.frozen() {
            return false;
        }
        if let Some(accepted) = self.route_field_action(event, window, cx) {
            return accepted;
        }
        let Some(entry) = self.semantic_actions.get(&event.target) else {
            return false;
        };
        if !entry.enabled.get() {
            return false;
        }
        if self.environment.is_some()
            && !matches!(
                entry.key.as_str(),
                "close-environment" | "cancel-environment" | "save-environment"
            )
        {
            return false;
        }
        let action = entry.action.clone();
        let focus = entry.focus.clone();
        match &event.kind {
            SemanticActionKind::Focus => {
                focus.focus(window);
                cx.notify();
            }
            SemanticActionKind::Click => self.act(action, window, cx),
            SemanticActionKind::Expand if matches!(action, controls::Action::Profiles) => {
                if !self.profile_menu {
                    self.act(action, window, cx);
                }
            }
            SemanticActionKind::Collapse if matches!(action, controls::Action::Profiles) => {
                if self.profile_menu {
                    self.act(action, window, cx);
                }
            }
            _ => return false,
        }
        true
    }
    pub(super) fn decorate_control(
        &mut self,
        key: &str,
        label: &str,
        action: controls::Action,
        focus: FocusHandle,
        element: gpui::Stateful<gpui::Div>,
    ) -> semantic_control::Control {
        let enabled = Rc::new(Cell::new(true));
        let semantic = self.control_semantic(key, label, &action, enabled.clone(), focus);
        semantic_control::Control::new(element, semantic, enabled)
    }
    pub(super) fn category_semantic_id(&self) -> Option<SemanticId> {
        self.semantic_id(&format!("panel/{}", self.category.label()))
    }
    pub(super) fn semantic_id(&self, key: &str) -> Option<SemanticId> {
        let binding = self.semantic.as_ref()?;
        match binding.frame.id_for(&format!("settings/{key}")) {
            Ok(id) => Some(id),
            Err(error) => {
                binding.frame.reject(error);
                None
            }
        }
    }
    pub(super) fn control_semantic(
        &mut self,
        key: &str,
        label: &str,
        action: &controls::Action,
        enabled: Rc<Cell<bool>>,
        focus: FocusHandle,
    ) -> Option<(SemanticFrame, SemanticNode)> {
        let binding = self.semantic.as_ref()?;
        let id = match binding.frame.id_for(&format!("settings/control/{key}")) {
            Ok(id) => id,
            Err(error) => {
                binding.frame.reject(error);
                return None;
            }
        };
        let parent = if self.environment.is_some()
            && matches!(
                key,
                "close-environment" | "cancel-environment" | "save-environment"
            ) {
            self.environment_semantic_id().unwrap_or(binding.root)
        } else {
            self.category_semantic_id().unwrap_or(binding.root)
        };
        let mut node = SemanticNode::new(
            id,
            Some(parent),
            Role::Button,
            self.control_name(key, label, action),
        );
        if let controls::Action::Remote(operation) = action
            && let Some((_, device_id)) = operation.client_access_target()
        {
            node.data.set_description(format!("Device ID: {device_id}"));
        }
        node.focus = Some(focus.clone());
        node.data.add_action(accesskit::Action::Click);
        if !enabled.get() {
            node.data.set_disabled();
        }
        if self.environment.is_some()
            && !matches!(
                key,
                "close-environment" | "cancel-environment" | "save-environment"
            )
        {
            node.data.set_disabled();
            enabled.set(false);
        }
        self.semantic_actions.insert(
            id,
            Invocation {
                key: key.into(),
                action: action.clone(),
                enabled,
                focus,
            },
        );
        Some((binding.frame.clone(), node))
    }
    pub(super) fn semantic_root<E: IntoElement>(&self, child: E) -> AnyElement {
        match &self.semantic {
            Some(binding) => binding
                .frame
                .wrap(
                    SemanticNode::new(binding.root, Some(binding.parent), Role::Pane, "Settings"),
                    child,
                )
                .into_any_element(),
            None => child.into_any_element(),
        }
    }
    pub(super) fn semantic_element<E: IntoElement>(
        &self,
        key: &str,
        role: Role,
        label: impl Into<String>,
        child: E,
    ) -> AnyElement {
        let Some(binding) = &self.semantic else {
            return child.into_any_element();
        };
        match binding.frame.id_for(&format!("settings/{key}")) {
            Ok(id) => binding
                .frame
                .wrap(
                    SemanticNode::new(id, Some(binding.root), role, label),
                    child,
                )
                .into_any_element(),
            Err(error) => {
                binding.frame.reject(error);
                child.into_any_element()
            }
        }
    }
    pub(super) fn semantic_dialog<E: IntoElement>(&self, child: E) -> AnyElement {
        match (&self.semantic, self.environment_semantic_id()) {
            (Some(binding), Some(id)) => {
                let mut node = SemanticNode::new(
                    id,
                    Some(binding.root),
                    Role::Dialog,
                    "Environment · All terminals",
                );
                node.data.set_modal();
                binding.frame.wrap(node, child).into_any_element()
            }
            _ => child.into_any_element(),
        }
    }
    pub(super) fn environment_semantic_id(&self) -> Option<SemanticId> {
        let binding = self.semantic.as_ref()?;
        let environment = self.environment.as_ref()?;
        match binding.frame.id_for(&format!(
            "settings/environment/{:?}",
            environment.text.entity_id()
        )) {
            Ok(id) => Some(id),
            Err(error) => {
                binding.frame.reject(error);
                None
            }
        }
    }
    fn control_name(&self, key: &str, label: &str, action: &controls::Action) -> String {
        use controls::Action;
        match action {
            Action::CloseSettings => "Close settings".into(),
            Action::SelectCategory(category) if label.is_empty() => category.label().into(),
            Action::SelectCategory(_) => label.into(),
            Action::ToggleUpdates => "Automatic updates".into(),
            Action::Accent(color) => format!("Use {color}"),
            Action::Background(color) => format!("Use background {color}"),
            Action::ChooseColor(kind) => match kind {
                api::ColorKind::Accent => "Accent color",
                api::ColorKind::Background => "Background color",
            }
            .into(),
            Action::SaveColor(kind) => format!(
                "Save custom {} color",
                match kind {
                    api::ColorKind::Accent => "accent",
                    api::ColorKind::Background => "background",
                }
            ),
            Action::Profiles => "Default shell".into(),
            Action::Environment => "Add Environment Variables (all terminals)".into(),
            Action::ToggleAgent(id) | Action::RemoveAgent(id) => {
                let name = self
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.value.process_rules.iter().find(|rule| rule.id == *id))
                    .map(|rule| rules::builtin_label(rule).unwrap_or(&rule.label))
                    .unwrap_or("program");
                format!(
                    "{} {name}",
                    if matches!(action, Action::ToggleAgent(_)) {
                        "Track"
                    } else {
                        "Remove"
                    }
                )
            }
            Action::Ssh(ssh::Operation::Edit(id)) => {
                let name = self
                    .ssh
                    .profiles
                    .as_ref()
                    .and_then(|profiles| profiles.iter().find(|profile| profile.id == *id))
                    .map(|profile| profile.name.as_str())
                    .unwrap_or("connection");
                format!(
                    "Edit {}{name}",
                    if key.starts_with("ssh-row-") {
                        "SSH connection "
                    } else {
                        ""
                    }
                )
            }
            Action::Ssh(ssh::Operation::Remove(id)) => format!(
                "Remove {}",
                self.ssh
                    .profiles
                    .as_ref()
                    .and_then(|profiles| profiles.iter().find(|profile| profile.id == *id))
                    .map(|profile| profile.name.as_str())
                    .unwrap_or("connection")
            ),
            Action::Ssh(ssh::Operation::ChooseKey) => "Choose key file".into(),
            Action::Ssh(ssh::Operation::ClearKey) => "Clear key file".into(),
            Action::Remote(remote::Operation::ToggleEnabled) => "Allow remote access".into(),
            Action::Remote(remote::Operation::Forget(id)) => {
                let name = self
                    .remote
                    .state
                    .as_ref()
                    .and_then(|state| state.peers.iter().find(|peer| peer.id == *id))
                    .map_or("device", |peer| peer.hostname.as_str());
                format!("Forget {name}")
            }
            Action::Remote(operation) => match operation.client_access_target() {
                Some((verb, _)) => format!(
                    "{verb} {}",
                    self.remote
                        .client_access_name(operation)
                        .unwrap_or("device")
                ),
                None => label.into(),
            },
            Action::CancelEnvironment if key == "close-environment" => "Close dialog".into(),
            _ => label.into(),
        }
    }
}
