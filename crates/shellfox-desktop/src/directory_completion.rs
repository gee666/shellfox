use super::paths;

#[derive(Default)]
pub(crate) struct Completion {
    matches: Vec<String>,
    cycle: Option<usize>,
    active: usize,
    accepted: bool,
    pub(crate) dismissed: bool,
    pub(crate) ticket: u64,
}

impl Completion {
    pub(crate) fn cancel(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
    }

    pub(crate) fn reset(&mut self) {
        self.cancel();
        self.matches.clear();
        self.cycle = None;
        self.active = 0;
        self.accepted = false;
        self.dismissed = false;
    }

    pub(crate) fn dismiss(&mut self) {
        self.reset();
        self.dismissed = true;
    }

    pub(crate) fn reopen(&mut self) -> bool {
        if !self.dismissed && !self.accepted {
            return false;
        }
        self.cancel();
        self.dismissed = false;
        self.accepted = false;
        true
    }

    pub(crate) fn arrive(&mut self, matches: Vec<String>) {
        self.matches = matches;
        self.cycle = None;
        self.active = 0;
        self.accepted = false;
    }

    pub(crate) fn visible(&self, input: &str) -> &[String] {
        if self.dismissed || matches!(self.matches.as_slice(), [only] if exact(input, only)) {
            &[]
        } else {
            &self.matches
        }
    }

    pub(crate) fn active(&self) -> usize {
        self.active
    }

    pub(crate) fn move_active(&mut self, input: &str, up: bool) -> bool {
        let len = self.visible(input).len();
        if len == 0 {
            return false;
        }
        self.active = (self.active + if up { len - 1 } else { 1 }) % len;
        self.accepted = false;
        true
    }

    pub(crate) fn choose(&mut self, input: &str, index: Option<usize>) -> Option<String> {
        if index.is_none() && self.accepted {
            return None;
        }
        let index = index.unwrap_or(self.active);
        self.visible(input).get(index)?;
        self.accept(index)
    }

    pub(crate) fn tab(&mut self) -> Option<String> {
        let len = self.matches.len();
        if len == 0 || len == 1 && self.accepted {
            return None;
        }
        self.accept(self.cycle.map_or(0, |index| (index + 1) % len))
    }

    fn accept(&mut self, index: usize) -> Option<String> {
        let value = self.matches.get(index)?.clone();
        self.cancel();
        self.cycle = Some(index);
        self.active = index;
        self.accepted = true;
        Some(value)
    }
}

fn exact(input: &str, only: &str) -> bool {
    let separator = |ch| ch == '/' || only.ends_with('\\') && ch == '\\';
    paths::unquote(input).trim_end_matches(separator) == only.trim_end_matches(separator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_wrap_and_enter_accepts_only_once() {
        let mut list = Completion::default();
        list.arrive(vec!["work/".into(), "workshop/".into()]);
        assert_eq!(list.visible("work").len(), 2);
        assert!(list.move_active("work", true));
        assert_eq!(list.choose("work", None).as_deref(), Some("workshop/"));
        assert!(list.choose("workshop/", None).is_none());
        assert_eq!(list.tab().as_deref(), Some("work/"));
        assert_eq!(list.tab().as_deref(), Some("workshop/"));
        list.dismiss();
        assert!(list.visible("work").is_empty());
    }

    #[test]
    fn sole_exact_match_is_hidden_but_tab_can_complete_it_and_then_query_children() {
        let mut list = Completion::default();
        list.arrive(vec!["a/".into()]);
        assert!(list.visible("'a'").is_empty());
        assert_eq!(list.tab().as_deref(), Some("a/"));
        assert!(list.tab().is_none());
        assert!(!exact("test", "test\\/"));
        assert!(exact(r"'C:\Users\'", r"C:\Users\"));
    }
}
