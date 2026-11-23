use crate::{owner::Owner, presentation::PresentationResult, presentation_api::*, *};

impl Owner {
    pub(crate) fn presentation_command(
        &mut self,
        view: PresentationLease,
        action: PresentationAction,
    ) -> Result<Response, RuntimeError> {
        match view {
            PresentationLease::Controller(lease) => {
                self.delivery.check(lease).map_err(RuntimeError::control)?
            }
            PresentationLease::Observer(observer) => self.observers.check(observer)?,
        }
        if let PresentationAction::ResizeExternal { cols, rows } = action {
            if self.source_kind != SourceKind::ExternalBytes {
                return Err(RuntimeError {
                    code: ErrorCode::Unsupported,
                    message: "Only passive external views have local layout resize.".into(),
                });
            }
            self.limits.validate_size(cols, rows)?;
            self.canonical_resize(cols, rows)?;
            self.replies()?;
            return Ok(Response::Applied);
        }
        let changed = matches!(
            action,
            PresentationAction::Scroll { .. }
                | PresentationAction::FollowTail
                | PresentationAction::Selection { .. }
                | PresentationAction::SelectionAtVersion { .. }
        );
        let focus = match action {
            PresentationAction::Frame { focused }
                if matches!(view, PresentationLease::Controller(_))
                    && self.source_kind == SourceKind::OwnedPty
                    && self.phase == Phase::Active =>
            {
                Some(focused)
            }
            _ => None,
        };
        let result = self.presentations.action(
            &mut self.terminal,
            self.identity,
            self.delivery.current_sequence(),
            self.limits,
            view,
            action,
        )?;
        if let Some(focused) = focus {
            if let Some(bytes) =
                self.presentations
                    .focus_report(&mut self.terminal, view, focused)?
            {
                self.write(&bytes)?;
            }
        }
        if changed && matches!(view, PresentationLease::Controller(_)) {
            self.dirty = true;
        }
        Ok(match result {
            PresentationResult::Frame(frame) => Response::Presentation(frame),
            PresentationResult::Applied => Response::Applied,
            PresentationResult::Text(text) => Response::Text(text),
        })
    }
    pub(crate) fn follow_input(&mut self, lease: Lease) -> Result<(), RuntimeError> {
        self.presentations.follow_input(&mut self.terminal, lease)?;
        self.dirty = true;
        Ok(())
    }
}
