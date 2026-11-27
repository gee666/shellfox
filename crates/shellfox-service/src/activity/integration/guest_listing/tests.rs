use super::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

/// Generous: the full suite runs these beside many other threads.
const LOADED: Duration = Duration::from_secs(30);

fn target(distro: &str) -> GuestTarget {
    GuestTarget::Tab {
        distro: WslProfileId::parse(&format!("wsl:{distro}")).unwrap(),
        account: "me".into(),
    }
}
/// Counts listings; each waits until the test opens `gate`.
fn gated(gate: Arc<Mutex<bool>>, calls: Arc<AtomicUsize>, ok: bool) -> GuestListings {
    GuestListings::with_lister(Arc::new(move |_: &GuestTarget| {
        calls.fetch_add(1, Ordering::SeqCst);
        while !*gate.lock().unwrap() {
            std::thread::sleep(Duration::from_millis(2));
        }
        ok.then(|| GuestProcesses {
            processes: Vec::new(),
            complete: true,
        })
    }))
}
fn listed(evidence: &GuestEvidence) -> bool {
    matches!(evidence, GuestEvidence::Listed { .. })
}
/// Polls at `now_ms` until the pending listing lands.
fn settle(listings: &mut GuestListings, target: &GuestTarget, now_ms: u64) -> GuestEvidence {
    let deadline = Instant::now() + LOADED;
    loop {
        let evidence = listings.evidence(target, "m".into(), 0, now_ms);
        if listed(&evidence) || Instant::now() > deadline {
            return evidence;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn listings_run_off_the_caller_one_at_a_time_per_target() {
    let (gate, calls) = (Arc::new(Mutex::new(false)), Arc::new(AtomicUsize::new(0)));
    let mut listings = gated(gate.clone(), calls.clone(), true);
    let wsl = target("Debian");
    // The caller never waits for the guest.
    for now in [0, 2_000, 4_000] {
        assert!(!listed(&listings.evidence(&wsl, "m".into(), 0, now)));
    }
    // However long the listing thread takes to start, only one runs.
    let deadline = Instant::now() + LOADED;
    while calls.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!listed(&listings.evidence(&wsl, "m".into(), 0, 6_000)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    *gate.lock().unwrap() = true;
    let GuestEvidence::Listed { marker, .. } = settle(&mut listings, &wsl, 4_000) else {
        panic!("listing")
    };
    assert_eq!(marker, "m");
    // Another distribution gets its own listing.
    let other = target("Ubuntu");
    assert!(listed(&settle(&mut listings, &other, 4_000)));
    assert_eq!(listings.slots.len(), 2);
}

#[test]
fn old_failed_or_pre_refresh_listings_are_unavailable() {
    let (gate, calls) = (Arc::new(Mutex::new(true)), Arc::new(AtomicUsize::new(0)));
    let mut listings = gated(gate.clone(), calls.clone(), true);
    let wsl = target("Debian");
    assert!(listed(&settle(&mut listings, &wsl, 1_000)));
    // An expiry refresh needs a listing started after it.
    assert!(!listed(&listings.evidence(&wsl, "m".into(), 1_001, 1_500)));
    // A hung refresh keeps the last listing only while it is recent.
    *gate.lock().unwrap() = false;
    assert!(listed(&listings.evidence(&wsl, "m".into(), 0, 2_000)));
    assert!(!listed(&listings.evidence(
        &wsl,
        "m".into(),
        0,
        1_000 + MAX_AGE_MS + 1
    )));
    *gate.lock().unwrap() = true;

    let mut failing = gated(gate, calls, false);
    let deadline = Instant::now() + LOADED;
    while failing
        .slots
        .get(&wsl)
        .is_none_or(|slot| slot.latest.is_none())
        && Instant::now() < deadline
    {
        failing.evidence(&wsl, "m".into(), 0, 0);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!listed(&failing.evidence(&wsl, "m".into(), 0, 0)));
}

#[test]
fn unused_targets_are_dropped_and_their_count_is_bounded() {
    let (gate, calls) = (Arc::new(Mutex::new(true)), Arc::new(AtomicUsize::new(0)));
    let mut listings = gated(gate, calls, true);
    for index in 0..MAX_TARGETS {
        listings.evidence(&target(&format!("d{index}")), "m".into(), 0, 0);
    }
    let extra = target("extra");
    assert!(!listed(&listings.evidence(&extra, "m".into(), 0, 0)));
    assert!(!listings.slots.contains_key(&extra));
    listings.retain(|kept| *kept == target("d0"));
    assert_eq!(listings.slots.len(), 1);
    assert!(listed(&settle(&mut listings, &extra, 0)));
}

#[test]
fn only_wsl_tabs_get_guest_evidence_and_forgetting_drops_their_listing() {
    use crate::activity::integration::{
        InspectionScope, RuntimeInspectionState, RuntimeProcessInspector,
    };
    use crate::activity::ProcessInspector;
    use shellfox_core::{Generation, SessionId, TabId};
    let identity = || shellfox_runtime::Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let (gate, calls) = (Arc::new(Mutex::new(true)), Arc::new(AtomicUsize::new(0)));
    let mut state = RuntimeInspectionState::with_guests(gated(gate, calls, true));
    let (wsl, local, unbound) = (identity(), identity(), identity());
    state.watch_guest(wsl, target("Debian")).unwrap();
    state.watch(local, InspectionScope::LocalWindows).unwrap();
    state.watch(unbound, InspectionScope::Guest).unwrap();
    let runtimes = std::collections::HashMap::<TabId, shellfox_runtime::Runtime>::new();
    let deadline = Instant::now() + LOADED;
    let marker = loop {
        let mut inspector = RuntimeProcessInspector::new(&runtimes, &mut state, 0);
        assert!(inspector.guest(local).is_none());
        assert!(matches!(
            inspector.guest(unbound),
            Some(GuestEvidence::Unavailable)
        ));
        match inspector.guest(wsl) {
            Some(GuestEvidence::Listed { marker, .. }) => break marker,
            _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
            _ => panic!("no guest listing"),
        }
    };
    // A WSL tab's marker is its tab UUID.
    assert_eq!(marker, wsl.tab_id.to_string());
    // The listing went with its last tab: a new tab there waits for its own.
    state.forget(wsl);
    let next = identity();
    state.watch_guest(next, target("Debian")).unwrap();
    let mut inspector = RuntimeProcessInspector::new(&runtimes, &mut state, 0);
    assert!(matches!(
        inspector.guest(next),
        Some(GuestEvidence::Unavailable)
    ));
}

#[test]
fn running_distributions_merge_into_one_listing() {
    let listing = |complete, markers: &[&str]| GuestProcesses {
        processes: markers
            .iter()
            .map(|marker| shellfox_terminal::wsl::GuestProcess {
                marker: (*marker).into(),
                accessible: true,
                executable: Some("/usr/bin/claude".into()),
                process_name: None,
                argv: None,
            })
            .collect(),
        complete,
    };
    let merged = merge(
        [
            Some(listing(true, &["a"])),
            Some(listing(true, &["b", "c"])),
        ]
        .into_iter(),
    );
    assert!(merged.complete);
    let markers: Vec<_> = merged.processes.iter().map(|p| p.marker.as_str()).collect();
    assert_eq!(markers, ["a", "b", "c"]);
    // A distribution that couldn't be listed, or listed partly, may hide one.
    assert!(!merge([Some(listing(true, &["a"])), None].into_iter()).complete);
    assert!(!merge([Some(listing(false, &[]))].into_iter()).complete);
}
