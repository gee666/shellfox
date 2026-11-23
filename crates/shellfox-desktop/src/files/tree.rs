use crate::root::Root;
use gpui::{Context, FocusHandle, ScrollHandle};
use shellfox_files::{EntryKind, FileEntry, FileList};
use std::{cell::RefCell, cmp::Ordering, collections::HashMap};

pub(crate) enum Branch {
    Loading,
    Ready(FileList),
    Failed(String),
}

pub(crate) struct Tree {
    pub branches: HashMap<String, (u64, Branch)>,
    pub focus: RefCell<HashMap<String, FocusHandle>>,
    pub scroll: ScrollHandle,
    pub scrollbar: super::scrollbar::Scrollbar,
    pub keyboard_focus: bool,
    pub loading: bool,
    next_request: u64,
}

impl Tree {
    pub fn new() -> Self {
        Self {
            branches: HashMap::new(),
            focus: RefCell::default(),
            scroll: ScrollHandle::new(),
            scrollbar: Default::default(),
            keyboard_focus: false,
            loading: false,
            next_request: 0,
        }
    }

    pub fn begin_reload(&mut self) {
        self.reset(false);
        self.loading = true;
    }

    pub(crate) fn cancel_loading(&mut self) {
        self.loading = false;
        self.branches
            .retain(|_, (_, branch)| !matches!(branch, Branch::Loading));
    }

    pub fn reset(&mut self, preserve_scroll: bool) {
        self.branches.clear();
        self.focus.get_mut().clear();
        if !preserve_scroll {
            self.scroll = ScrollHandle::new();
            self.scrollbar = Default::default();
        }
    }

    fn load(&mut self, path: String) -> u64 {
        self.next_request += 1;
        self.branches
            .insert(path, (self.next_request, Branch::Loading));
        self.next_request
    }

    fn collapse(&mut self, path: &str) {
        let in_branch = |key: &str| {
            key == path
                || key
                    .strip_prefix(path)
                    .is_some_and(|suffix| suffix.starts_with(['/', '\\']))
        };
        self.branches.retain(|key, _| !in_branch(key));
        self.focus
            .get_mut()
            .retain(|key, _| key == path || !in_branch(key));
    }
}

impl Root {
    pub(crate) fn toggle_file_branch(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(files) = &mut self.files.panel else {
            return;
        };
        if files.tree.branches.contains_key(&path) {
            files.tree.collapse(&path);
            cx.notify();
        } else {
            self.load_file_branch(path, cx);
        }
    }

    pub(crate) fn load_file_branch(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files.panel else {
            return;
        };
        let request = files.tree.load(path.clone());
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        let target = path.clone();
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.list(&target).await });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                if !root.files_result_current(epoch, &backend) {
                    return;
                }
                let Some((id, branch)) = root
                    .files
                    .panel
                    .as_mut()
                    .and_then(|files| files.tree.branches.get_mut(&path))
                else {
                    return;
                };
                if *id != request {
                    return;
                }
                let mut prepared = match result {
                    Ok(listing) => Branch::Ready(listing),
                    Err(error) => Branch::Failed(error.to_string()),
                };
                if backend
                    .commit_if_current(|| std::mem::swap(branch, &mut prepared))
                    .is_ok()
                {
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}

pub(crate) fn compare_entries(a: &FileEntry, b: &FileEntry) -> Ordering {
    (b.kind == EntryKind::Directory)
        .cmp(&(a.kind == EntryKind::Directory))
        .then_with(|| natural_cmp(&a.name, &b.name))
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    use icu_collator::{
        Collator, CollatorBorrowed, CollatorPreferences,
        options::{CollatorOptions, Strength},
        preferences::CollationNumericOrdering,
    };
    thread_local! {
        static COLLATOR: Option<CollatorBorrowed<'static>> = {
            let locale = sys_locale::get_locale().and_then(|tag| tag.parse::<icu_locale::Locale>().ok());
            let numeric = |mut prefs: CollatorPreferences| {
                prefs.numeric_ordering = Some(CollationNumericOrdering::True);
                prefs
            };
            let mut options = CollatorOptions::default();
            options.strength = Some(Strength::Primary);
            Collator::try_new(numeric(locale.map(Into::into).unwrap_or_default()), options)
                .or_else(|_| Collator::try_new(numeric(CollatorPreferences::default()), options)).ok()
        };
    }
    COLLATOR.with(|c| c.as_ref().map_or_else(|| a.cmp(b), |c| c.compare(a, b)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural_order_compares_entire_digit_runs_without_integer_overflow() {
        assert_eq!(natural_cmp("item2", "item10"), Ordering::Less);
        assert_eq!(natural_cmp("item001", "item1"), Ordering::Equal);
        assert_eq!(natural_cmp("reportA", "Reporta"), Ordering::Equal);
        assert_eq!(
            natural_cmp("item9999999999999999999999", "item10000000000000000000000"),
            Ordering::Less
        );
    }
    #[test]
    fn reload_drops_expanded_branches_and_invalidates_pending_reads() {
        let mut tree = Tree::new();
        let old = tree.load("/folder/branch".into());

        tree.begin_reload();
        assert!(tree.loading);
        assert!(tree.branches.is_empty());

        tree.cancel_loading();
        assert!(!tree.loading);
        assert_ne!(tree.load("/folder/branch".into()), old);
    }

    #[test]
    fn collapse_drops_descendants_but_not_sibling_prefixes_and_invalidates_reads() {
        let mut tree = Tree::new();
        let old = tree.load(r"C:\parent\a".into());
        tree.load(r"C:\parent\a\b".into());
        tree.load(r"C:\parent\aa".into());
        tree.collapse(r"C:\parent\a");
        assert_eq!(tree.branches.len(), 1);
        assert!(tree.branches.contains_key(r"C:\parent\aa"));
        assert_ne!(tree.load(r"C:\parent\a".into()), old);
    }
}
