//! Local owner/cursor fencing fixtures, not authenticated Runtime lease proof.
use super::*;
fn event() -> m::MouseEvent {
    m::MouseEvent {
        action: m::MouseAction::Press,
        button: Some(m::MouseButton::Left),
        modifiers: m::Modifiers::default(),
        x_px: 8.0,
        y_px: 17.0,
        any_button_pressed: true,
    }
}
fn hints() -> m::InputHints {
    m::InputHints {
        user_initiated: true,
        activity_hint: true,
        settled_at: None,
    }
}
#[test]
fn mouse_freezes_actual_owner_layout_before_queue_and_rejects_stale_or_unready_view() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mut manager, transport) = manager().await;
        let mut value = attachment(&transport);
        let m::Attachment::Attached(attached) = &mut value else {
            panic!("fixture attached")
        };
        attached.input = Some(m::ControlRef {
            tab_id: id(5),
            generation: id(6),
            control_id: id(8),
            control_epoch: 1,
        });
        attached.input_state = m::InputState::Owned;
        attached.layout = Some(m::LayoutRef {
            tab_id: id(5),
            generation: id(6),
            layout_id: id(9),
            layout_epoch: 1,
        });
        transport.ready(value);
        let mut request = request();
        request.control = m::ControlRequest::IfFree;
        manager.attach(request).await.unwrap();
        let owner = manager.owner_view();
        assert!(owner.admit_mouse(event(), 9, 18, hints()).is_err());
        owner.pump().unwrap();
        assert!(owner.admit_mouse(event(), 9, 18, hints()).is_err());
        manager.acknowledge_consumed().await.unwrap();
        let wrong = owner.clone();
        assert!(matches!(
            std::thread::spawn(move || wrong.admit_mouse(event(), 9, 18, hints()))
                .join()
                .unwrap(),
            Err(ManagerRecoveryError::Engine(crate::Status::WrongThread))
        ));
        let sent = owner.admit_mouse(event(), 9, 18, hints()).unwrap();
        assert!(matches!(
            manager.execute_input(sent).await.unwrap(),
            m::Reply::Failure(_)
        ));
        let wire = transport.0.lock().unwrap().calls.last().unwrap().clone();
        let geometry: m::GeometryStamp =
            serde_json::from_value(wire["payload"]["input"]["geometry"].clone()).unwrap();
        assert_eq!(
            (
                geometry.cols,
                geometry.rows,
                geometry.layout_revision,
                geometry.cell_width_px,
                geometry.cell_height_px
            ),
            (20, 5, 1, 9, 18)
        );
        let pending = owner.admit_mouse(event(), 9, 18, hints()).unwrap();
        let forged = m::Input::Mouse {
            event: event(),
            geometry,
        };
        assert!(manager.admit_input(forged, hints()).is_err());
        let delivery = manager
            .state
            .lock()
            .unwrap()
            .cursor
            .as_ref()
            .unwrap()
            .delivery()
            .clone();
        transport.event(m::Event::Layout {
            delivery,
            layout_revision: 2,
            cols: 21,
            rows: 5,
            recovery_required: true,
            last_sequence: 0,
        });
        let context = ManagerPollContext::capture(manager.client.as_ref(), &[&manager]).unwrap();
        context.poll(manager.client.as_ref()).await.unwrap();
        assert!(owner.admit_mouse(event(), 9, 18, hints()).is_err());
        let calls = transport.calls();
        assert!(manager.execute_input(pending).await.is_err());
        assert_eq!(transport.calls(), calls);
        manager.close().unwrap();
    });
}
