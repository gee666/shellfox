use super::{Backend, TerminalView, runtime::Pending};
use gpui::Context;
use shellfox_ghostty_vt::{Selection, ViewportPoint};
use shellfox_runtime::{Command, PresentationAction, PresentationLease};

fn checked_selection(
    expected: PresentationLease,
    current: PresentationLease,
    displayed: PresentationLease,
    sampled_version: u64,
    action: PresentationAction,
    cols: u16,
    rows: u16,
) -> anyhow::Result<PresentationAction> {
    anyhow::ensure!(
        expected == current && expected == displayed,
        "Accessible selection targets a stale presentation lease"
    );
    let PresentationAction::SelectionAtVersion { version, selection } = action else {
        anyhow::bail!("Accessible selection requires its sampled presentation version");
    };
    anyhow::ensure!(
        version == sampled_version,
        "Accessible selection sample is no longer displayed"
    );
    let valid = |point: ViewportPoint| point.x < cols && point.y < rows;
    let bounds = match selection {
        Some(Selection::Range { start, end, .. }) => valid(start) && valid(end),
        Some(Selection::Word(point) | Selection::Line(point)) => valid(point),
        Some(Selection::All) | None => true,
    };
    anyhow::ensure!(
        bounds,
        "Accessible selection coordinates are outside the accepted view"
    );
    // Forward the caller's sampled version unchanged. The owner also checks its
    // current source revision/cache, closing the cross-queue race.
    Ok(PresentationAction::SelectionAtVersion { version, selection })
}
impl TerminalView {
    pub(crate) fn apply_semantic_selection(
        &mut self,
        expected: PresentationLease,
        action: PresentationAction,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.failed && !self.closing && !self.detached && !self.control_blocked,
            "Terminal surface is unavailable"
        );
        let Backend::Attached(connection) = &self.backend else {
            anyhow::bail!("Accessible selection requires an attached presentation");
        };
        let frame = self
            .presentation_frame()
            .ok_or_else(|| anyhow::anyhow!("An accepted presentation frame is required"))?;
        let generation = match &connection.access {
            super::runtime::native::Access::Control(a) => a.generation,
            super::runtime::native::Access::Observer(a) => a.generation,
        };
        anyhow::ensure!(
            frame.identity.tab_id == connection.tab && frame.identity.generation == generation,
            "Accessible selection frame identity is stale"
        );
        let action = match checked_selection(
            expected,
            connection.access.presentation_lease(),
            frame.view,
            frame.version,
            action,
            frame.screen.screen.cols,
            frame.screen.screen.rows,
        ) {
            Ok(action) => action,
            Err(error) => {
                self.semantic_binding = None;
                cx.notify();
                return Err(error);
            }
        };
        let next = self
            .cursor_reset_epoch
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("View cursor reset epoch exhausted"))?;
        anyhow::ensure!(
            self.submit(
                Command::Presentation {
                    view: expected,
                    action
                },
                Pending::SemanticSelection
            ),
            "Accessible selection was not accepted by the bounded presentation queue"
        );
        self.cursor_reset_epoch = next;
        cx.notify();
        Ok(())
    }
}
#[cfg(test)]
mod tests;
