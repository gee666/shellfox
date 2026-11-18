//! View-book ownership state, not a grant/request ledger or authorization.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MailboxCustody {
    Provisional,
    Adopted,
}
/// Borrows the exact validated book through the central Service adoption.
/// Commit cannot allocate, look up a different book, fail or race its removal.
pub(crate) struct MailboxAdoption<'a>(&'a mut MailboxCustody);
impl<'a> MailboxAdoption<'a> {
    pub(crate) fn provisional(state: &'a mut MailboxCustody) -> Option<Self> {
        if *state == MailboxCustody::Provisional {
            Some(Self(state))
        } else {
            None
        }
    }
    pub(crate) fn commit(self) {
        *self.0 = MailboxCustody::Adopted;
    }
}
