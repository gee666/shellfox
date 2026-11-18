//! Pure reference/custody codec checks. No actual Native controller grant.
use super::*;
#[test]
fn pure_controller_reference_codec_never_proves_a_native_grant_or_geometry() {
    let identity = identity();
    let role = Role::Controller(Lease {
        controller: 19,
        epoch: 7,
    });
    let book = ManagerDelivery::new(identity, role).unwrap();
    let control = book.control().unwrap().clone();
    let layout = book.layout().unwrap().clone();
    assert_eq!(control.control_epoch, 7);
    assert_eq!(layout.layout_epoch, 7);
    assert_ne!(control.control_id, layout.layout_id);
    assert_ne!(control.control_id, book.reference().delivery_id);
    assert_eq!(
        book.ready_control(&control).unwrap_err(),
        Failure::RecoveryRequired
    );
    let geometry = wire::GeometryStamp {
        recovery_id: wire::Id::new(),
        layout_revision: 7,
        cols: 80,
        rows: 24,
        cell_width_px: 8,
        cell_height_px: 16,
    };
    assert_eq!(
        book.geometry(&control, &geometry).unwrap_err(),
        Failure::RecoveryRequired
    );
    let mut mailboxes = ManagerMailboxes::default();
    mailboxes.insert_offer(book).unwrap();
    mailboxes.prepare_adoption(identity, role).unwrap().commit();
    mailboxes.revoke_controllers(identity);
    let book = mailboxes.get(identity, role).unwrap();
    assert!(book.control().is_none());
    assert!(book.layout().is_none());
    assert_eq!(book.ready_control(&control).unwrap_err(), Failure::Stale);
    assert!(book.poll(1024, 1024).unwrap().iter().all(|event| matches!(
        event,
        wire::Event::Revoked {
            reason: wire::RevokedReason::ControlLost,
            ..
        }
    )));
    mailboxes.revoke(identity, role, wire::RevokedReason::ViewDetached);
    // Pure exact-identity cleanup routing, not actual source-destruction proof.
    mailboxes.source_destroyed(Identity {
        generation: Generation::new(),
        ..identity
    });
    assert!(mailboxes.get(identity, role).is_some());
    mailboxes.source_destroyed(identity);
    assert!(mailboxes.get(identity, role).is_none());
}

#[test]
fn pure_controller_custody_codec_keeps_exact_lease_when_cleanup_is_not_admitted() {
    let identity = identity();
    let old_lease = Lease {
        controller: 29,
        epoch: 11,
    };
    let role = Role::Controller(old_lease);
    let book = ManagerDelivery::new(identity, role).unwrap();
    let reference = book.reference().clone();
    let mut mailboxes = ManagerMailboxes::default();
    mailboxes.insert_offer(book).unwrap();
    mailboxes.prepare_adoption(identity, role).unwrap().commit();
    mailboxes.revoke_controllers(identity);
    // No new Runtime claim/cleanup command is issued in this pure codec case.
    // Rejection, cancellation or Full before cleanup admission must not erase it.
    for _ in 0..3 {
        assert_eq!(
            mailboxes.retirements(),
            vec![(identity, Role::Controller(old_lease))]
        );
        assert_eq!(
            mailboxes.get(identity, role).unwrap().role(),
            Role::Controller(old_lease)
        );
        assert!(mailboxes.get(identity, role).unwrap().control().is_none());
        assert!(mailboxes.get(identity, role).unwrap().layout().is_none());
    }
    assert!(matches!(
        mailboxes.revocation(&reference),
        Some(wire::RevokedReason::ControlLost)
    ));
}

#[test]
fn pure_provisional_handoff_codec_has_exactly_one_cleanup_owner() {
    let identity = identity();
    let role = Role::Controller(Lease {
        controller: 31,
        epoch: 13,
    });
    let mut mailboxes = ManagerMailboxes::default();
    mailboxes
        .insert_offer(ManagerDelivery::new(identity, role).unwrap())
        .unwrap();
    mailboxes.revoke_controllers(identity);
    assert!(
        mailboxes.retirements().is_empty(),
        "central offer, not provisional book, owns cleanup"
    );
    assert!(
        mailboxes.prepare_adoption(identity, role).is_none(),
        "revoked provisional book cannot adopt"
    );
    mailboxes.discard_unadopted(identity, role).unwrap();
    mailboxes
        .insert_offer(ManagerDelivery::new(identity, role).unwrap())
        .unwrap();
    // Pure view-state commit only: no Runtime/controller grant is proved here.
    mailboxes.prepare_adoption(identity, role).unwrap().commit();
    assert!(mailboxes.discard_unadopted(identity, role).is_err());
    mailboxes.revoke_controllers(identity);
    assert_eq!(mailboxes.retirements(), vec![(identity, role)]);
}
