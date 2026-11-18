use super::SemanticError;
use accesskit::NodeId;
use std::collections::{BTreeMap, BTreeSet};

const MAX_KEY_BYTES: usize = 64 * 1024;

enum Reservation {
    Single(NodeId),
    Group(Vec<NodeId>),
}

/// Exact keys, not hashes. Issued IDs and reservations survive every paint reset.
pub(super) struct SemanticIds {
    keys: BTreeMap<String, Reservation>,
    used: BTreeSet<NodeId>,
    next: Option<u64>,
}

impl SemanticIds {
    pub(super) fn new(root: NodeId) -> Self {
        Self {
            keys: BTreeMap::new(),
            used: BTreeSet::from([root]),
            next: Some(1),
        }
    }

    fn validate(key: &str) -> Result<(), SemanticError> {
        if key.is_empty() || key.len() > MAX_KEY_BYTES {
            return Err(SemanticError::InvalidIdKey);
        }
        Ok(())
    }

    pub(super) fn id_for(&mut self, key: &str) -> Result<NodeId, SemanticError> {
        Self::validate(key)?;
        match self.keys.get(key) {
            Some(Reservation::Single(id)) => return Ok(*id),
            Some(Reservation::Group(_)) => return Err(SemanticError::IdKeyConflict),
            None => (),
        }
        let (ids, next) = self.plan(1)?;
        let id = ids[0];
        self.used.insert(id);
        self.next = next;
        self.keys.insert(key.to_owned(), Reservation::Single(id));
        Ok(id)
    }

    pub(super) fn reserve_ids(
        &mut self,
        key: &str,
        count: usize,
    ) -> Result<Vec<NodeId>, SemanticError> {
        Self::validate(key)?;
        if count == 0 || count > super::tree::MAX_NODES {
            return Err(SemanticError::InvalidIdCount);
        }
        match self.keys.get(key) {
            Some(Reservation::Group(ids)) if ids.len() == count => return Ok(ids.clone()),
            Some(_) => return Err(SemanticError::IdKeyConflict),
            None => (),
        }
        let (ids, next) = self.plan(count)?;
        self.used.extend(ids.iter().copied());
        self.next = next;
        self.keys
            .insert(key.to_owned(), Reservation::Group(ids.clone()));
        Ok(ids)
    }

    fn plan(&self, count: usize) -> Result<(Vec<NodeId>, Option<u64>), SemanticError> {
        let mut next = self.next;
        let mut result = Vec::with_capacity(count);
        while result.len() < count {
            let value = next.ok_or(SemanticError::IdExhausted)?;
            next = value.checked_add(1);
            let id = NodeId(value);
            if !self.used.contains(&id) {
                result.push(id);
            }
        }
        Ok((result, next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_keys_never_alias_and_groups_skip_root_and_single_ids() {
        let mut ids = SemanticIds::new(NodeId(1));
        let first = ids.id_for("session/ab").unwrap();
        let second = ids.id_for("session/a/b").unwrap();
        assert_eq!(first, ids.id_for("session/ab").unwrap());
        assert_ne!(first, second);
        let rows = ids.reserve_ids("terminal/generation/rows", 250).unwrap();
        let unique: BTreeSet<_> = rows.iter().copied().collect();
        assert_eq!(unique.len(), 250);
        assert!(!unique.contains(&NodeId(1)));
        assert!(!unique.contains(&first));
        assert!(!unique.contains(&second));
        assert_eq!(
            rows,
            ids.reserve_ids("terminal/generation/rows", 250).unwrap()
        );
    }

    #[test]
    fn conflicting_key_kinds_and_group_sizes_are_errors_not_fallback_ids() {
        let mut ids = SemanticIds::new(NodeId(1));
        ids.id_for("single").unwrap();
        assert!(matches!(
            ids.reserve_ids("single", 1),
            Err(SemanticError::IdKeyConflict)
        ));
        ids.reserve_ids("rows", 2).unwrap();
        assert!(matches!(
            ids.id_for("rows"),
            Err(SemanticError::IdKeyConflict)
        ));
        assert!(matches!(
            ids.reserve_ids("rows", 3),
            Err(SemanticError::IdKeyConflict)
        ));
        assert!(matches!(ids.id_for(""), Err(SemanticError::InvalidIdKey)));
        assert!(matches!(
            ids.reserve_ids("empty", 0),
            Err(SemanticError::InvalidIdCount)
        ));
    }

    #[test]
    fn exhaustion_is_checked_and_failed_group_allocation_is_atomic() {
        let mut ids = SemanticIds::new(NodeId(1));
        ids.next = Some(u64::MAX);
        assert!(matches!(
            ids.reserve_ids("two", 2),
            Err(SemanticError::IdExhausted)
        ));
        assert_eq!(ids.next, Some(u64::MAX));
        assert_eq!(ids.id_for("last").unwrap(), NodeId(u64::MAX));
        assert!(matches!(
            ids.id_for("overflow"),
            Err(SemanticError::IdExhausted)
        ));
        assert_eq!(ids.id_for("last").unwrap(), NodeId(u64::MAX));
    }
}
