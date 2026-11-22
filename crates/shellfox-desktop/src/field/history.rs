use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_BYTES: usize = 24 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;

pub(super) struct EditState {
    pub(super) text: Arc<str>,
    pub(super) anchor: usize,
    pub(super) cursor: usize,
}

#[derive(Default)]
pub(super) struct History {
    past: VecDeque<EditState>,
    future: VecDeque<EditState>,
    typing: Option<(Instant, usize)>,
}

impl History {
    pub(super) fn record(&mut self, before: EditState, typed_to: Option<usize>) {
        self.future.clear();
        let now = Instant::now();
        let continues = typed_to.is_some()
            && self.typing.is_some_and(|(when, offset)| {
                now.duration_since(when) < Duration::from_millis(600) && offset == before.cursor
            });
        if !continues {
            push_bounded(&mut self.past, before);
        }
        self.typing = typed_to.map(|offset| (now, offset));
    }

    pub(super) fn transition(&mut self, redo: bool, current: EditState) -> Option<EditState> {
        self.break_group();
        let (from, to) = if redo {
            (&mut self.future, &mut self.past)
        } else {
            (&mut self.past, &mut self.future)
        };
        let state = from.pop_back()?;
        push_bounded(to, current);
        Some(state)
    }

    pub(super) fn break_group(&mut self) {
        self.typing = None;
    }
}

fn push_bounded(states: &mut VecDeque<EditState>, state: EditState) {
    states.push_back(state);
    let mut bytes: usize = states.iter().map(|state| state.text.len()).sum();
    while (states.len() > MAX_ENTRIES || bytes > MAX_BYTES)
        && let Some(removed) = states.pop_front()
    {
        bytes -= removed.text.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalesced_typing_undo_and_redo_restore_source_offsets() {
        let state = |text: &'static str| EditState {
            text: text.into(),
            anchor: text.len(),
            cursor: text.len(),
        };
        let mut history = History::default();
        history.record(state(""), Some(1));
        history.record(state("x"), Some(2));
        assert_eq!(&*history.transition(false, state("xy")).unwrap().text, "");
        assert_eq!(&*history.transition(true, state("")).unwrap().text, "xy");
    }
}
