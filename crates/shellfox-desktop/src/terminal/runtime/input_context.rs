use super::super::TerminalView;
use shellfox_ghostty_vt::{MouseAction, key::KeyAction};
use shellfox_runtime::Command;
use shellfox_service::native::NativeInputContext;

pub(super) fn input_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Key { .. } | Command::Paste { .. } | Command::Mouse { .. }
    )
}
fn input_hint(command: &Command) -> bool {
    match command {
        Command::Key { event, .. } => event.action != KeyAction::Release,
        Command::Paste { text, .. } => !text.is_empty(),
        Command::Mouse { event, .. } => {
            event.action != MouseAction::Motion
                || event.any_button_pressed
                || event.button.is_some()
        }
        _ => false,
    }
}
impl TerminalView {
    pub(in crate::terminal) fn frozen_input(
        &self,
        command: &Command,
    ) -> Option<NativeInputContext> {
        self.captured_settlement
            .clone()
            .map(|captured_settlement| NativeInputContext {
                captured_settlement,
                user_initiated: true,
                hint: input_hint(command),
            })
    }
    pub(in crate::terminal) fn cancel_held_input(
        &mut self,
        pointer_only: bool,
    ) -> Result<(), &'static str> {
        let input =
            self.captured_settlement
                .clone()
                .map(|captured_settlement| NativeInputContext {
                    captured_settlement,
                    user_initiated: false,
                    hint: false,
                });
        self.outbox.cancel_with_context(pointer_only, input)
    }
}
