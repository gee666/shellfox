use super::{ProfileMenu, Root};
use crate::accessibility::{Action, Role, SemanticAction, SemanticActionKind, SemanticNode};
use gpui::{AnyElement, Context, Div, Stateful, Window, prelude::*};

impl ProfileMenu {
    fn key(&self) -> String {
        format!("workspace/profile-menu/{:?}", self.focus)
    }

    fn choice_key(&self, index: usize) -> String {
        format!("{}/{index}", self.key())
    }
}

impl Root {
    pub(super) fn profile_menu_element(
        &self,
        menu: &ProfileMenu,
        items: Stateful<Div>,
    ) -> AnyElement {
        let frame = self.semantics.context.frame();
        match frame.id_for(&menu.key()) {
            Ok(id) => frame
                .wrap(
                    SemanticNode::new(id, Some(frame.root_id()), Role::Menu, "Terminal profiles"),
                    items,
                )
                .into_any_element(),
            Err(error) => {
                frame.reject(error);
                items.into_any_element()
            }
        }
    }

    pub(super) fn profile_choice_element(
        &self,
        menu: &ProfileMenu,
        index: usize,
        row: Stateful<Div>,
        enabled: bool,
    ) -> AnyElement {
        let frame = self.semantics.context.frame();
        let ids = frame
            .id_for(&menu.key())
            .and_then(|parent| frame.id_for(&menu.choice_key(index)).map(|id| (parent, id)));
        let (parent, id) = match ids {
            Ok(ids) => ids,
            Err(error) => {
                frame.reject(error);
                return row.into_any_element();
            }
        };
        let choice = &menu.choices[index];
        let mut node = SemanticNode::new(id, Some(parent), Role::MenuItem, choice.label.clone());
        if choice.hint() != choice.label {
            node.data.set_description(choice.hint());
        }
        if enabled {
            node.data.add_action(Action::Click);
            node.focus = Some(choice.focus.clone());
        } else {
            node.data.set_disabled();
        }
        frame.wrap(node, row).into_any_element()
    }

    pub(crate) fn handle_profile_menu_semantic_action(
        &mut self,
        action: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(menu) = &mut self.tabs.profile_menu else {
            return false;
        };
        let frame = self.semantics.context.frame();
        if frame.focus_handle(action.target).is_none()
            || self.dialogs.user_action_pending
            || self.close.closing
        {
            return false;
        }
        let Some(index) = (0..menu.choices.len())
            .find(|&index| frame.id_for(&menu.choice_key(index)).ok() == Some(action.target))
        else {
            return false;
        };
        match action.kind {
            SemanticActionKind::Click => self.choose_terminal_profile(index, window, cx),
            SemanticActionKind::Focus => {
                menu.keyboard_selection = Some(index);
                menu.choices[index].focus.focus(window);
                cx.notify();
            }
            _ => return false,
        }
        true
    }
}
