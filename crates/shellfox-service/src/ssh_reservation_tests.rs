use super::*;
struct ReservedProbe(Arc<AtomicBool>);
impl ProcessRunner for ReservedProbe {
    fn run(&self, _: &ProcessRequest) -> shellfox_ssh::Result<ProcessOutput> {
        assert!(
            self.0.load(Ordering::SeqCst),
            "Probe ran before the owner reserved its source."
        );
        Ok(ProcessOutput {
            success: true,
            stdout: format!("fixture.invalid ssh-ed25519 {PUBLIC}\n").into_bytes(),
            stderr: vec![],
        })
    }
}
#[test]
fn source_is_reserved_before_preparation_and_local_attach_guard_stays_closed_through_cancel() {
    let fixture = Fixture::new();
    let reserved = Arc::new(AtomicBool::new(false));
    let mut manager = fixture.with_probe(Arc::new(ReservedProbe(reserved.clone())));
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter {
        reservation_seen: Some(reserved),
        ..Default::default()
    };
    let id = connect(&mut manager, &validator, &mut starter);
    let tab = manager.connections[&id].status.tab;
    let target = manager.source_target(tab).unwrap();
    assert_eq!(target.connection(), id);
    assert_eq!(target.tab(), tab);
    assert_eq!(starter.targets.get(&tab), Some(&target));
    assert!(manager.ensure_local_start_allowed(tab).is_err());
    assert!(starter
        .reserve_target(8, SourceConnectTarget::new(Uuid::new_v4(), tab))
        .is_err());
    drive(&mut manager, &validator, &mut starter, |m| {
        matches!(m.connections[&id].status.state, State::AwaitingHost(_))
    });
    manager.detach_controller(7);
    assert!(manager.ensure_local_start_allowed(tab).is_err());
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
    assert!(manager.ensure_local_start_allowed(tab).is_ok());
    assert!(starter.targets.is_empty());
    assert_eq!(starter.starts, 0);
    assert_eq!(starter.releases, 1);
}
#[test]
fn failed_reservation_starts_no_worker_or_runtime() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter {
        deny_reserve: true,
        ..Default::default()
    };
    let p = input();
    let profile = p.id;
    manager
        .request(7, Request::Save(p), &validator, &mut starter)
        .unwrap();
    let tab = shellfox_core::TabId::new();
    assert!(manager
        .request(
            7,
            Request::Connect {
                profile,
                tab,
                cols: 80,
                rows: 24
            },
            &validator,
            &mut starter
        )
        .is_err());
    assert!(manager.connections.is_empty());
    assert!(manager.source_target(tab).is_none());
    assert_eq!(starter.starts, 0);
    assert!(manager.is_closed());
}
