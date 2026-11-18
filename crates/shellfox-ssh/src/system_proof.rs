//! A remote command emits this only after OpenSSH authenticates and opens a channel.
//! It is a bounded byte matcher, not a second terminal emulator or output parser.
use std::fmt;
use uuid::Uuid;

#[derive(Clone)]
pub struct AuthenticationProof {
    marker: Vec<u8>,
    matched: usize,
    confirmed: bool,
}
impl AuthenticationProof {
    pub(crate) fn new() -> Self {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        Self {
            marker: format!("\x1b]777;shellfox-auth={token}\x07").into_bytes(),
            matched: 0,
            confirmed: false,
        }
    }
    pub(crate) fn command(&self) -> String {
        // The generated token contains only ASCII hex. There is no user-controlled shell text.
        let text = String::from_utf8_lossy(&self.marker);
        format!("printf '\\033{}\\007'; ", &text[1..text.len() - 1])
    }
    pub fn confirmed(&self) -> bool {
        self.confirmed
    }
    /// Accept only the same challenge actually matched by an authoritative live tap.
    pub fn accept_confirmation(&mut self, other: &Self) -> bool {
        if other.confirmed && self.marker == other.marker {
            self.confirmed = true;
            self.matched = 0;
            true
        } else {
            false
        }
    }
    pub fn observe(&mut self, bytes: &[u8]) -> bool {
        for &byte in bytes {
            if self.confirmed {
                break;
            }
            if self.marker[self.matched] == byte {
                self.matched += 1;
            } else {
                self.matched = usize::from(byte == self.marker[0]);
            }
            if self.matched == self.marker.len() {
                self.confirmed = true;
                self.matched = 0;
            }
        }
        self.confirmed
    }
}
impl fmt::Debug for AuthenticationProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthenticationProof([HOST-ONLY])")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_receipt_must_be_confirmed_and_challenge_bound() {
        let mut expected = AuthenticationProof::new();
        let mut live = expected.clone();
        let other = AuthenticationProof::new();
        assert!(!expected.accept_confirmation(&live));
        live.observe(&expected.marker);
        assert!(live.confirmed());
        assert!(!expected.accept_confirmation(&other));
        assert!(expected.accept_confirmation(&live));
    }
    #[test]
    fn authentication_is_not_spawn_and_matches_only_the_exact_bounded_marker() {
        let mut proof = AuthenticationProof::new();
        assert!(!proof.observe(b"Started Authenticated prompt without proof"));
        let marker = proof.marker.clone();
        let mut other = AuthenticationProof::new();
        assert!(!other.observe(&marker));
        for byte in &marker[..marker.len() - 1] {
            assert!(!proof.observe(&[*byte]));
        }
        assert!(proof.observe(&marker[marker.len() - 1..]));
        assert!(proof.observe(b"later terminal output"));
        assert!(proof.command().starts_with("printf '\\033]777;"));
    }
}
