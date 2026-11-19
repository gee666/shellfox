use shellfox_ghostty_vt::{CursorVisual, PresentationFrame, Screen};
#[cfg(test)]
use shellfox_runtime::ScreenFrame;
use shellfox_runtime::ViewFrame;
use std::{ops::Deref, rc::Rc, sync::Arc};

pub(crate) struct PeerFrame {
    pub(crate) rendered: PresentationFrame,
    pub(crate) cursor: CursorVisual,
    pub(crate) selection: Option<shellfox_ghostty_vt::PresentationSelection>,
    pub(crate) sequence: u64,
}

/// Local sessions share the service frame. Remote v1 sessions render locally.
#[derive(Clone)]
pub(crate) enum SharedScreen {
    Empty,
    Fixture(Rc<Screen>),
    Peer(Rc<PeerFrame>),
    Native(Arc<ViewFrame>),
}

impl Deref for SharedScreen {
    type Target = Screen;
    fn deref(&self) -> &Screen {
        static EMPTY: Screen = Screen {
            cols: 0,
            rows: 0,
            cells: Vec::new(),
            cursor: shellfox_ghostty_vt::Cursor {
                x: 0,
                y: 0,
                visible: false,
            },
        };
        match self {
            Self::Empty => &EMPTY,
            Self::Fixture(screen) => screen,
            Self::Peer(frame) => &frame.rendered.screen,
            Self::Native(frame) => &frame.screen.screen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_view_clone_shares_the_same_native_cells_without_a_mirror_parser() {
        let mut terminal = shellfox_ghostty_vt::Terminal::new(2, 2).unwrap();
        terminal.feed(b"ab").unwrap();
        let identity = shellfox_runtime::Identity {
            session_id: shellfox_core::SessionId::new(),
            tab_id: shellfox_core::TabId::new(),
            generation: shellfox_core::Generation::new(),
        };
        let mut view = terminal.new_presentation_view().unwrap();
        terminal
            .view_select(&mut view, Some(shellfox_ghostty_vt::Selection::All))
            .unwrap();
        let rendered = terminal.view_render(&mut view).unwrap();
        let frame = Arc::new(ViewFrame {
            identity,
            sequence: 1,
            version: 1,
            view: shellfox_runtime::PresentationLease::Controller(shellfox_runtime::Lease {
                controller: 1,
                epoch: 1,
            }),
            screen: Arc::new(ScreenFrame {
                identity,
                sequence: 1,
                screen: rendered.screen,
                mouse_tracking: false,
            }),
            row_metadata: rendered.row_metadata.into(),
            selection: terminal.view_selection(&mut view).unwrap(),
            cursor_visual: terminal.cursor_visual().unwrap(),
            top_row: rendered.top_row,
            total_rows: rendered.total_rows,
        });
        assert!(
            frame.selection.is_some(),
            "actual tracked view selection must be retained"
        );
        let screen = SharedScreen::Native(frame.clone());
        let cloned = screen.clone();
        assert!(std::ptr::eq(&screen.cells[0], &cloned.cells[0]));
        assert_eq!(Arc::strong_count(&frame), 3);
    }
}
