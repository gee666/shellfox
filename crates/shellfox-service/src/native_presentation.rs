use super::{cache::ViewKey, dispatch::control_key, *};
use anyhow::{ensure, Context, Result};
use shellfox_runtime::{Command, PresentationAction, PresentationLease};

#[derive(Clone, Debug)]
pub enum NativeViewAttachment {
    Controller(Attachment),
    Observer(NativeObserverAttachment),
}
#[derive(Clone, Debug)]
pub enum NativePresentation {
    Frame {
        focused: bool,
    },
    Scroll {
        delta: isize,
    },
    FollowTail,
    ResizeExternal {
        cols: u16,
        rows: u16,
    },
    Selection {
        selection: Option<Selection>,
    },
    SelectionAtVersion {
        version: u64,
        selection: Option<Selection>,
    },
    Copy,
    Hyperlink {
        point: ViewportPoint,
    },
}
impl NativePresentation {
    fn runtime(self) -> PresentationAction {
        match self {
            Self::Frame { focused } => PresentationAction::Frame { focused },
            Self::Scroll { delta } => PresentationAction::Scroll { delta },
            Self::FollowTail => PresentationAction::FollowTail,
            Self::ResizeExternal { cols, rows } => {
                PresentationAction::ResizeExternal { cols, rows }
            }
            Self::Selection { selection } => PresentationAction::Selection { selection },
            Self::SelectionAtVersion { version, selection } => {
                PresentationAction::SelectionAtVersion { version, selection }
            }
            Self::Copy => PresentationAction::Copy,
            Self::Hyperlink { point } => PresentationAction::Hyperlink { point },
        }
    }
}
impl crate::Service {
    pub(super) fn native_presentation(
        &mut self,
        tab: TabId,
        controller: u64,
        attachment: NativeViewAttachment,
        action: NativePresentation,
    ) -> Result<NativeResponse> {
        let (key, view) = match attachment {
            NativeViewAttachment::Controller(attachment) => {
                self.native_control(tab, controller, &attachment)?;
                (
                    control_key(controller, tab),
                    PresentationLease::Controller(self.lease(tab, &attachment)?),
                )
            }
            NativeViewAttachment::Observer(attachment) => {
                let lease = self.native_observer(tab, controller, &attachment)?;
                (
                    ViewKey {
                        controller,
                        tab,
                        observer: true,
                    },
                    PresentationLease::Observer(lease),
                )
            }
        };
        match self.call(
            tab,
            Command::Presentation {
                view,
                action: action.runtime(),
            },
        )? {
            Response::Presentation(frame) => {
                let grant = &self
                    .native
                    .views
                    .get(&key)
                    .context("Native presentation is detached.")?
                    .grant;
                ensure!(
                    frame.identity.tab_id == tab
                        && frame.identity.generation == grant.generation()
                        && frame.view == view,
                    "Native presentation identity or view mismatch."
                );
                self.native.capture(key, frame)?;
                Ok(NativeResponse::Frame(self.native.offer(key)?))
            }
            response @ (Response::Applied | Response::Text(_)) => {
                Ok(NativeResponse::Terminal(response))
            }
            _ => anyhow::bail!("Runtime returned an invalid presentation response."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passive_external_layout_resize_forwards_exact_dimensions_not_pty_resize() {
        let action = NativePresentation::ResizeExternal { cols: 83, rows: 29 }.runtime();
        assert!(matches!(
            action,
            PresentationAction::ResizeExternal { cols: 83, rows: 29 }
        ));
    }
    #[test]
    fn guarded_selection_version_is_not_rewritten_into_an_ordinary_selection() {
        let action = NativePresentation::SelectionAtVersion {
            version: 73,
            selection: None,
        }
        .runtime();
        assert!(matches!(
            action,
            PresentationAction::SelectionAtVersion {
                version: 73,
                selection: None
            }
        ));
    }
}
