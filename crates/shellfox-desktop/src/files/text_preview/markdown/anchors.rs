use gpui::ScrollHandle;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Clone, Default)]
pub(super) struct Anchors {
    blocks: Rc<RefCell<HashMap<String, usize>>>,
    pub scroll: ScrollHandle,
}

impl Anchors {
    pub fn register(&self, id: String, index: usize) {
        if !id.is_empty() {
            self.blocks.borrow_mut().entry(id).or_insert(index);
        }
    }

    pub fn jump(&self, id: &str) -> bool {
        let Some(&index) = self.blocks.borrow().get(id) else {
            return false;
        };
        self.scroll.scroll_to_top_of_item(index);
        true
    }

    pub fn clear(&self) {
        self.blocks.borrow_mut().clear();
    }
}

pub(super) fn slug(text: &str) -> String {
    text.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(|character| {
            if character.is_whitespace() {
                Some('-')
            } else if character.is_alphanumeric() || matches!(character, '-' | '_') {
                Some(character)
            } else {
                None
            }
        })
        .collect()
}
