use shellfox_host::*;
use std::{path::PathBuf, sync::Arc};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("creation-receipts-{}", SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn input() -> CreateSession {
    CreateSession {
        request_id: SessionId::new(),
        title: None,
        cwd: "/work".into(),
        profile_id: ProfileId::new("cmd").unwrap(),
    }
}
fn receipt(id: SessionId) -> CreationRequestReceipt {
    CreationRequestReceipt::SessionCreated { session_id: id }
}
#[test]
fn duplicate_creation_global_selection_is_not_the_original_request_receipt() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let first = input();
    let second = input();
    assert_eq!(
        host.creation_request_receipt(first.request_id).unwrap(),
        CreationRequestReceipt::ReceiptUnavailable
    );
    let original = host
        .create_session_with_facts(
            first.clone(),
            SessionCreationFacts::new(SettingsAdapter::EmbeddedPty, SettingsShell::LoginShell),
        )
        .unwrap()
        .selection
        .session_id
        .unwrap();
    let latest = host.create_session(second.clone()).unwrap();
    let other = latest.selection.session_id.unwrap();
    assert_ne!(original, other);
    let repeated = host.create_session(first.clone()).unwrap();
    assert_eq!(repeated, latest);
    assert_eq!(repeated.selection.session_id, Some(other));
    assert_eq!(
        host.creation_request_receipt(first.request_id).unwrap(),
        receipt(original)
    );
    assert_eq!(
        host.creation_request_receipt(second.request_id).unwrap(),
        receipt(other)
    );
    assert_eq!(
        host.snapshot().unwrap(),
        latest,
        "lookup must not change selection/revision or infer a focus target"
    );
    drop(host);
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(
        host.creation_request_receipt(first.request_id).unwrap(),
        receipt(original)
    );
    assert_eq!(host.snapshot().unwrap().selection.session_id, Some(other));
}
#[test]
fn receipt_identifies_session_only_not_an_arbitrary_remaining_tab_or_runtime() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let request = input();
    let created = host.create_session(request.clone()).unwrap();
    let session = created.selection.session_id.unwrap();
    let first_tab = created.selection.tab_id.unwrap();
    let added = host
        .add_tab(
            session,
            AddTab {
                title: Some("later tab".into()),
                cwd: None,
                profile_id: ProfileId::new("pwsh").unwrap(),
            },
        )
        .unwrap();
    let later = added.selection.tab_id.unwrap();
    host.reorder_tabs(session, &[later, first_tab]).unwrap();
    host.close_tab(first_tab).unwrap();
    host.rename_session(session, "renamed").unwrap();
    assert_eq!(
        host.creation_request_receipt(request.request_id).unwrap(),
        receipt(session)
    );
    assert_eq!(host.snapshot().unwrap().sessions[0].tabs[0].id, later);
}
#[test]
fn deleted_or_never_recorded_receipts_are_explicitly_unavailable_not_current_selection() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let first = input();
    let id = host
        .create_session(first.clone())
        .unwrap()
        .selection
        .session_id
        .unwrap();
    let second = input();
    let other = host
        .create_session(second.clone())
        .unwrap()
        .selection
        .session_id
        .unwrap();
    host.delete_session(id).unwrap();
    let before = host.snapshot().unwrap();
    for request in [first.request_id, SessionId::new()] {
        assert_eq!(
            host.creation_request_receipt(request).unwrap(),
            CreationRequestReceipt::ReceiptUnavailable
        );
    }
    assert_eq!(
        host.creation_request_receipt(second.request_id).unwrap(),
        receipt(other)
    );
    assert_eq!(host.snapshot().unwrap(), before);
    drop(host);
    assert_eq!(
        Host::open(&scratch.0)
            .unwrap()
            .creation_request_receipt(first.request_id)
            .unwrap(),
        CreationRequestReceipt::ReceiptUnavailable
    );
}
struct Lost;
impl SettingsRuntime for Lost {
    fn normalize(&self, _: &Settings, candidate: Settings) -> Result<Settings> {
        Ok(candidate)
    }
    fn check_commit(&self) -> Result<()> {
        Err(Error::new(ErrorCode::Internal, "Native ownership lost."))
    }
}
#[test]
fn failed_creation_leaves_no_durable_receipt_even_when_another_session_is_selected() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let existing = host.create_session(input()).unwrap();
    host.install_settings_runtime(Arc::new(Lost)).unwrap();
    let request = input();
    assert!(host
        .create_session_with_facts(
            request.clone(),
            SessionCreationFacts::new(SettingsAdapter::EmbeddedPty, SettingsShell::LoginShell)
        )
        .is_err());
    assert_eq!(
        host.creation_request_receipt(request.request_id).unwrap(),
        CreationRequestReceipt::ReceiptUnavailable
    );
    assert_eq!(host.snapshot().unwrap(), existing);
}
