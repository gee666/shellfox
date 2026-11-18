// Only a read may hold one unsent New terminal intent. Other operations stay exclusive.
pub(crate) enum RefreshSlot<T> {
    Idle,
    Reading,
    Reserved(T),
}

impl<T> Default for RefreshSlot<T> {
    fn default() -> Self {
        Self::Idle
    }
}

impl<T> RefreshSlot<T> {
    pub(crate) fn reserve(&mut self, intent: T) -> bool {
        if !matches!(self, Self::Reading) {
            return false;
        }
        *self = Self::Reserved(intent);
        true
    }

    pub(crate) fn finish(&mut self) -> Option<T> {
        match std::mem::take(self) {
            Self::Reserved(intent) => Some(intent),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RefreshSlot;

    #[test]
    fn only_an_active_refresh_can_reserve_an_intent() {
        let mut slot = RefreshSlot::default();
        assert!(!slot.reserve("not refreshing"));
        slot = RefreshSlot::Reading;
        assert_eq!(slot.finish(), None);
        assert!(!slot.reserve("already finished"));
    }

    #[test]
    fn a_refresh_reserves_one_intent_and_hands_it_back_once() {
        let mut slot = RefreshSlot::Reading;
        assert!(slot.reserve("first click"));
        assert!(!slot.reserve("second click"));
        assert_eq!(slot.finish(), Some("first click"));
        assert_eq!(slot.finish(), None);
    }

    #[test]
    fn a_discarded_intent_is_not_replayed_by_the_next_refresh() {
        let mut slot = RefreshSlot::Reading;
        assert!(slot.reserve("original click"));
        let _ = slot.finish();
        slot = RefreshSlot::Reading;
        assert_eq!(slot.finish(), None);
    }
}
