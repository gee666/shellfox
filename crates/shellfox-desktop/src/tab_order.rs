pub(crate) fn move_tab<T: Copy + Eq>(
    tabs: impl Iterator<Item = (T, bool)>,
    source: T,
    target: T,
) -> Option<Vec<T>> {
    if source == target {
        return None;
    }
    let all: Vec<_> = tabs.collect();
    let mut visible: Vec<_> = all
        .iter()
        .filter_map(|&(id, movable)| movable.then_some(id))
        .collect();
    let from = visible.iter().position(|id| *id == source)?;
    let to = visible.iter().position(|id| *id == target)?;
    visible.remove(from);
    visible.insert(to, source);

    // The Host needs the complete permutation, including fixed slots.
    let mut visible = visible.into_iter();
    all.into_iter()
        .map(|(id, movable)| if movable { visible.next() } else { Some(id) })
        .collect()
}

pub(crate) fn move_tab_by<T: Copy + Eq>(
    tabs: impl Iterator<Item = (T, bool)>,
    source: T,
    offset: isize,
) -> Option<Vec<T>> {
    if offset == 0 {
        return None;
    }
    let all: Vec<_> = tabs.collect();
    let movable: Vec<_> = all
        .iter()
        .filter_map(|&(id, can_move)| can_move.then_some(id))
        .collect();
    let from = movable.iter().position(|id| *id == source)?;
    let to = from.checked_add_signed(offset)?;
    let target = *movable.get(to)?;
    move_tab(all.into_iter(), source, target)
}

#[cfg(test)]
mod tests {
    use super::{move_tab, move_tab_by};

    #[test]
    fn moves_to_the_original_target_index_in_either_direction() {
        let tabs = [(1, true), (2, true), (3, true), (4, true)];
        assert_eq!(move_tab(tabs.into_iter(), 1, 3), Some(vec![2, 3, 1, 4]));
        assert_eq!(move_tab(tabs.into_iter(), 4, 2), Some(vec![1, 4, 2, 3]));
    }

    #[test]
    fn keeps_archived_slots_in_the_complete_permutation() {
        let tabs = [(1, false), (2, true), (3, false), (4, true), (5, true)];
        assert_eq!(move_tab(tabs.into_iter(), 2, 5), Some(vec![1, 4, 3, 5, 2]));
        assert_eq!(move_tab(tabs.into_iter(), 5, 2), Some(vec![1, 5, 3, 2, 4]));
    }

    #[test]
    fn keyboard_step_moves_one_visible_tab_without_wrapping_or_crossing_fixed_slots() {
        let tabs = [(1, true), (2, false), (3, true), (4, true)];
        assert_eq!(move_tab_by(tabs.into_iter(), 3, -1), Some(vec![3, 2, 1, 4]));
        assert_eq!(move_tab_by(tabs.into_iter(), 3, 1), Some(vec![1, 2, 4, 3]));
        assert_eq!(move_tab_by(tabs.into_iter(), 1, -1), None);
        assert_eq!(move_tab_by(tabs.into_iter(), 4, 1), None);
        assert_eq!(move_tab_by(tabs.into_iter(), 3, 0), None);
    }

    #[test]
    fn rejects_noop_removed_or_archived_endpoints() {
        let tabs = [(1, true), (2, false), (3, true)];
        for (source, target) in [(1, 1), (2, 3), (1, 2), (4, 3), (1, 4)] {
            assert_eq!(move_tab(tabs.into_iter(), source, target), None);
        }
    }
}
