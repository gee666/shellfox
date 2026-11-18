//! Local transport rejection tests, not authenticated source/control grant proof.
use super::*;
#[test]
fn attach_response_cannot_reroute_another_tab_under_the_same_pair() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mut manager, transport) = manager().await;
        let before = old(&manager);
        let mut value = attachment(&transport);
        let m::Attachment::Attached(attached) = &mut value else {
            panic!()
        };
        attached.delivery.tab_id = id(8);
        transport.ready(value);
        assert!(manager.attach(request()).await.is_err());
        assert_eq!(progress(&manager), (0, 0));
        unchanged(&manager, &before);
        let calls = transport.calls();
        assert!(manager.acknowledge_consumed().await.is_err());
        assert_eq!(calls, transport.calls());
        // A correct response still goes through the original public request.
        transport.ready(attachment(&transport));
        manager.attach(request()).await.unwrap();
        manager.owner_view().pump().unwrap();
        manager.acknowledge_consumed().await.unwrap();
        manager.close().unwrap();
    });
}
#[test]
fn resume_response_requires_the_original_exact_generation_before_install_or_ack() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mut manager, transport) = manager().await;
        let before = old(&manager);
        let mut input = request();
        input.resume = Some(m::Resume {
            generation: id(9),
            after_sequence: 0,
        });
        transport.ready(attachment(&transport));
        assert!(manager.attach(input).await.is_err());
        assert_eq!(progress(&manager), (0, 0));
        unchanged(&manager, &before);
        let calls = transport.calls();
        assert!(manager.prepare_pending().is_err());
        assert!(manager.acknowledge_consumed().await.is_err());
        assert_eq!(calls, transport.calls());
        let mut correct = request();
        correct.resume = Some(m::Resume {
            generation: id(6),
            after_sequence: 0,
        });
        transport.ready(attachment(&transport));
        manager.attach(correct).await.unwrap();
        manager.owner_view().pump().unwrap();
        manager.acknowledge_consumed().await.unwrap();
        manager.close().unwrap();
    });
}
#[test]
fn safe_owner_selection_copy_tracking_and_scroll_keep_thread_and_lifetime_checks() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (manager, transport) = runtime.block_on(manager());
    old(&manager);
    let owner = manager.owner_view();
    let calls = transport.calls();
    owner
        .set_selection(Some(shellfox_ghostty_vt::Selection::All))
        .unwrap();
    assert!(owner
        .copy_selection()
        .unwrap()
        .unwrap()
        .contains("OLD VIEW"));
    owner.set_selection(None).unwrap();
    assert!(!owner.mouse_tracking().unwrap());
    assert!(owner
        .hyperlink_at(shellfox_ghostty_vt::ViewportPoint { x: 0, y: 0 })
        .unwrap()
        .is_none());
    owner.scroll(0).unwrap();
    assert!(owner.scroll(1_000_001).is_err());
    let wrong = owner.clone();
    assert!(matches!(
        std::thread::spawn(move || wrong.copy_selection())
            .join()
            .unwrap(),
        Err(ManagerRecoveryError::Engine(crate::Status::WrongThread))
    ));
    owner.close().unwrap();
    assert!(owner.copy_selection().is_err());
    assert_eq!(transport.calls(), calls);
}
