use super::SshOpenRejection;
use shellfox_core::SessionId;
/// This acknowledges only the local UI queue, never server admission,
/// authentication, process launch, readiness or handled-window activation.
#[must_use = "Root must handle local SSH admission and retain the original request ID"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SshOpenAdmission {
    Queued {
        request_id: SessionId,
    },
    Rejected {
        request_id: SessionId,
        reason: SshOpenRejection,
    },
}
#[derive(Default)]
pub(super) struct Intent {
    pending: Option<SessionId>,
}
pub(super) struct Completion<T> {
    pub request_id: SessionId,
    pub result: Result<T, String>,
}
impl Intent {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn admit(&mut self, request_id: SessionId, frozen: bool, busy: bool) -> SshOpenAdmission {
        let reason = if frozen {
            Some(SshOpenRejection::Frozen)
        } else if busy || self.pending.is_some() {
            Some(SshOpenRejection::Busy)
        } else {
            None
        };
        if let Some(reason) = reason {
            return SshOpenAdmission::Rejected { request_id, reason };
        }
        self.pending = Some(request_id);
        SshOpenAdmission::Queued { request_id }
    }
    pub fn finish<T>(
        &mut self,
        request_id: SessionId,
        result: Result<T, String>,
    ) -> Option<Completion<T>> {
        if self.pending != Some(request_id) {
            return None;
        }
        self.pending = None;
        Some(Completion { request_id, result })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_and_frozen_reject_before_any_rpc_and_do_not_replace_intent() {
        let mut intent = Intent::default();
        let old = SessionId::new();
        assert!(
            matches!(intent.admit(old,false,false),SshOpenAdmission::Queued{request_id}if request_id==old)
        );
        let mut rpc = 0;
        for (frozen, busy, reason) in [
            (false, true, SshOpenRejection::Busy),
            (true, false, SshOpenRejection::Frozen),
        ] {
            let next = SessionId::new();
            let result = intent.admit(next, frozen, busy);
            if matches!(result, SshOpenAdmission::Queued { .. }) {
                rpc += 1;
            }
            assert!(
                matches!(result,SshOpenAdmission::Rejected{request_id,reason:r}if request_id==next&&r==reason)
            );
        }
        assert_eq!(rpc, 0);
        assert!(intent.pending());
        assert!(intent.finish(old, Ok(())).is_some());
    }
    #[test]
    fn stale_success_or_failure_never_clears_current_intent() {
        let mut intent = Intent::default();
        let id = SessionId::new();
        let _ = intent.admit(id, false, false);
        assert!(intent.finish(SessionId::new(), Ok(())).is_none());
        assert!(
            intent
                .finish(SessionId::new(), Err::<(), _>("stale".into()))
                .is_none()
        );
        assert!(intent.pending());
        let failure = intent
            .finish(id, Err::<(), _>("unavailable".into()))
            .unwrap();
        assert_eq!(failure.request_id, id);
        assert_eq!(failure.result.unwrap_err(), "unavailable");
        assert!(!intent.pending());
    }
    #[test]
    fn success_keeps_original_request_not_a_session_derived_identifier() {
        let mut intent = Intent::default();
        let request = SessionId::new();
        let session = SessionId::new();
        let _ = intent.admit(request, false, false);
        let success = intent.finish(request, Ok(session)).unwrap();
        assert_eq!(success.request_id, request);
        assert_ne!(success.request_id, success.result.unwrap());
        assert!(!intent.pending());
        let newer = SessionId::new();
        let _ = intent.admit(newer, false, false);
        assert!(
            intent
                .finish(request, Err::<(), _>("late".into()))
                .is_none()
        );
        assert!(intent.pending());
    }
}
