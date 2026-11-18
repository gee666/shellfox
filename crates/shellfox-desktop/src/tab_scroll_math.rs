#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Viewport<Session, Tab> {
    pub(crate) session: Session,
    pub(crate) selected: Option<(Tab, usize)>,
    pub(crate) width: f32,
}

impl<Session: PartialEq, Tab: PartialEq> Viewport<Session, Tab> {
    pub(crate) fn reveal_left(
        &self,
        previous: Option<&Self>,
        scroll_left: f32,
        tab_edges: Option<(f32, f32)>,
        max_left: f32,
    ) -> Option<f32> {
        let left = match previous {
            Some(previous) if previous == self => return None,
            Some(previous) if previous.session == self.session => scroll_left,
            _ => 0.,
        };
        let left = match tab_edges {
            Some((start, _)) if start < left => start,
            Some((_, end)) if end > left + self.width => end - self.width,
            _ => left,
        };
        Some(left.clamp(0., max_left))
    }
}

#[cfg(test)]
mod tests {
    use super::Viewport;

    fn viewport() -> Viewport<u8, u8> {
        Viewport {
            session: 1,
            selected: Some((2, 3)),
            width: 160.,
        }
    }

    #[test]
    fn unchanged_viewport_leaves_manual_scroll_alone() {
        let view = viewport();
        assert_eq!(
            view.reveal_left(Some(&view), 100., Some((0., 90.)), 300.),
            None
        );
    }

    #[test]
    fn selection_order_and_width_changes_reveal_from_current_offset() {
        let before = viewport();
        for view in [
            Viewport {
                selected: Some((3, 3)),
                ..before
            },
            Viewport {
                selected: Some((2, 4)),
                ..before
            },
            Viewport {
                width: 120.,
                ..before
            },
        ] {
            assert_eq!(
                view.reveal_left(Some(&before), 100., Some((120., 210.)), 300.),
                Some(100.)
            );
        }
    }

    #[test]
    fn first_paint_and_session_switch_reveal_from_the_start() {
        let before = viewport();
        let view = Viewport {
            session: 4,
            ..before
        };
        assert_eq!(
            view.reveal_left(Some(&before), 100., Some((50., 140.)), 300.),
            Some(0.)
        );
        assert_eq!(
            view.reveal_left(None, 0., Some((200., 290.)), 300.),
            Some(130.)
        );
    }

    #[test]
    fn reveal_moves_only_as_far_as_needed() {
        let view = viewport();
        let before = Viewport {
            width: 200.,
            ..view
        };
        let reveal = |edges| view.reveal_left(Some(&before), 100., edges, 300.);
        assert_eq!(reveal(Some((130., 220.))), Some(100.));
        assert_eq!(reveal(Some((60., 150.))), Some(60.));
        assert_eq!(reveal(Some((200., 290.))), Some(130.));
        assert_eq!(reveal(None), Some(100.));
    }

    #[test]
    fn reveal_stays_within_the_scrollable_extent() {
        let view = viewport();
        assert_eq!(
            view.reveal_left(None, 0., Some((200., 290.)), 80.),
            Some(80.)
        );
        assert_eq!(view.reveal_left(None, 0., Some((0., 90.)), 0.), Some(0.));
    }
}
