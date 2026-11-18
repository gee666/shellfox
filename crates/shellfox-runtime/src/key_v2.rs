//! Owned FIFO input over the safe pinned producer vocabulary. No wire/OS mapping.
use crate::{owner::Owner, *};
#[cfg(test)]
#[path = "key_v2_tests.rs"]
mod tests;
use shellfox_ghostty_vt::KeyEventV2;
pub use shellfox_ghostty_vt::{InvalidKeyV2, KeyActionV2, ModifiersV2, PhysicalKey};

/// Owns every admission field until the owner synchronously invokes the encoder.
/// Physical identity, layout text, side/lock state and composition are not guessed.
#[derive(Clone, PartialEq, Eq)]
pub struct OwnedKeyEventV2 {
    pub physical: PhysicalKey,
    pub action: KeyActionV2,
    pub modifiers: ModifiersV2,
    pub consumed_modifiers: ModifiersV2,
    pub text: String,
    pub unshifted_codepoint: Option<char>,
    pub composing: bool,
}
impl std::fmt::Debug for OwnedKeyEventV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedKeyEventV2")
            .field("physical", &self.physical)
            .field("action", &self.action)
            .field("modifiers", &self.modifiers)
            .field("consumed_modifiers", &self.consumed_modifiers)
            .field("text_bytes", &self.text.len())
            .field(
                "has_unshifted_codepoint",
                &self.unshifted_codepoint.is_some(),
            )
            .field("composing", &self.composing)
            .finish()
    }
}
impl From<KeyEventV2<'_>> for OwnedKeyEventV2 {
    fn from(event: KeyEventV2<'_>) -> Self {
        Self {
            physical: event.physical,
            action: event.action,
            modifiers: event.modifiers,
            consumed_modifiers: event.consumed_modifiers,
            text: event.text.to_owned(),
            unshifted_codepoint: event.unshifted_codepoint,
            composing: event.composing,
        }
    }
}
impl OwnedKeyEventV2 {
    fn borrowed(&self) -> KeyEventV2<'_> {
        KeyEventV2 {
            physical: self.physical,
            action: self.action,
            modifiers: self.modifiers,
            consumed_modifiers: self.consumed_modifiers,
            text: &self.text,
            unshifted_codepoint: self.unshifted_codepoint,
            composing: self.composing,
        }
    }
}
impl Owner {
    pub(crate) fn key_v2(
        &mut self,
        request: RequestId,
        lease: Lease,
        event: OwnedKeyEventV2,
    ) -> Result<Response, RuntimeError> {
        self.active()?;
        // As with Key, a stamped epoch cannot bypass its pending recovery/layout.
        self.controller_side_effects(lease, false)?;
        let bytes = self
            .terminal
            .encode_key_v2(&event.borrowed())
            .map_err(crate::commands::engine)?;
        let action = match event.action {
            KeyActionV2::Press => shellfox_ghostty_vt::KeyAction::Press,
            KeyActionV2::Repeat => shellfox_ghostty_vt::KeyAction::Repeat,
            KeyActionV2::Release => shellfox_ghostty_vt::KeyAction::Release,
        };
        let result = self.accept_input(
            request,
            &bytes,
            InputKind::Key {
                action,
                has_committed_text: !event.composing && !event.text.is_empty(),
            },
        )?;
        if event.action != KeyActionV2::Release && !bytes.is_empty() {
            self.follow_accepted_input(lease);
        }
        Ok(result)
    }
}
