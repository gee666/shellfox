use super::*;

fn steps(request: &Request) -> usize {
    match &request.command {
        Command::Wheel { steps, .. } => usize::from(*steps),
        command if wheel(command) => 1,
        _ => 0,
    }
}

#[test]
fn fast_spin_behind_a_slow_host_is_accepted_in_a_few_requests() {
    let mut queue = Outbox::default();
    let mut host = Transport::new();
    host.fill();
    for _ in 0..500 {
        queue
            .enqueue(
                mouse(MouseAction::Press, MouseButton::WheelDown, 10.),
                Pending::Applied,
            )
            .unwrap();
        host.flush(&mut queue);
    }
    let requests = host.finish(&mut queue);
    let wheel: Vec<_> = requests.iter().filter(|r| steps(r) > 0).collect();
    assert_eq!(wheel.iter().map(|r| steps(r)).sum::<usize>(), 500);
    assert!(wheel.len() <= 2, "{} wheel requests", wheel.len());
    assert_eq!(queue.wheel_entries, 0);
}

#[test]
fn coalesced_wheel_keeps_its_place_between_keys() {
    let mut queue = Outbox::default();
    let mut host = Transport::new();
    host.fill();
    let spin = |queue: &mut Outbox| {
        for _ in 0..300 {
            queue
                .enqueue(
                    mouse(MouseAction::Press, MouseButton::WheelUp, 10.),
                    Pending::Applied,
                )
                .unwrap();
        }
    };
    spin(&mut queue);
    queue
        .enqueue(key(KeyAction::Press), Pending::Applied)
        .unwrap();
    spin(&mut queue);
    let order: Vec<_> = host
        .finish(&mut queue)
        .iter()
        .filter_map(|request| match &request.command {
            Command::Key { .. } => Some(0),
            _ => Some(steps(request)).filter(|steps| *steps > 0),
        })
        .collect();
    assert_eq!(order, [256, 44, 0, 256, 44]);
}
