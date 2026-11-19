use shellfox_core::SessionId;
use std::collections::{HashMap, HashSet};

/// Each desktop window and the aggregate remote views contribute independently.
/// Owners without any renderer keep the headless polling policy until a view reports.
#[derive(Default)]
pub(super) struct Visibility {
    desktop: HashMap<u64, HashSet<SessionId>>,
    remote: HashSet<SessionId>,
    reported: bool,
}

impl Visibility {
    pub(super) fn desktop(&mut self, controller: u64, ids: Vec<SessionId>) {
        self.reported = true;
        self.desktop.insert(controller, ids.into_iter().collect());
    }

    pub(super) fn detach(&mut self, controller: u64) {
        self.desktop.remove(&controller);
    }

    pub(super) fn remote(&mut self, ids: HashSet<SessionId>) {
        self.reported = true;
        self.remote = ids;
    }

    pub(super) fn eligible(
        &self,
        watched: impl Iterator<Item = SessionId>,
    ) -> Option<Vec<SessionId>> {
        self.reported.then(|| {
            let mut eligible: Vec<_> = watched
                .filter(|id| {
                    self.remote.contains(id) || self.desktop.values().any(|ids| ids.contains(id))
                })
                .collect();
            eligible.sort_unstable();
            eligible.dedup();
            eligible
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_windows_and_remote_views_without_polling_unknown_ids() {
        let a = SessionId::new();
        let b = SessionId::new();
        let c = SessionId::new();
        let unknown = SessionId::new();
        let mut v = Visibility::default();
        assert_eq!(v.eligible([a, b, c].into_iter()), None);
        v.desktop(1, vec![a, unknown]);
        v.desktop(2, vec![b]);
        v.remote(HashSet::from([a, c]));
        let mut expected = vec![a, b, c];
        expected.sort_unstable();
        assert_eq!(v.eligible([a, b, c, a].into_iter()), Some(expected));
        v.desktop(1, Vec::new());
        v.detach(2);
        v.remote(HashSet::from([c]));
        assert_eq!(v.eligible([a, b, c].into_iter()), Some(vec![c]));
        v.remote(HashSet::new());
        assert_eq!(v.eligible([a, b, c].into_iter()), Some(vec![]));
    }
}
