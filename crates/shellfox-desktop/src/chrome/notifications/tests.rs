use super::*;

#[test]
fn machine_notices_keep_only_the_first_line_and_at_most_200_utf16_units() {
    assert_eq!(
        machine_notice_line("Cannot refresh this peer.\r\nTransport details"),
        "Cannot refresh this peer."
    );
    let multibyte = format!("{}🦊two", "x".repeat(199));
    assert_eq!(machine_notice_line(&multibyte), "x".repeat(199));
    assert_eq!(machine_notice_line(&"x".repeat(205)).len(), 200);
}

#[test]
fn source_key_and_error_code_keep_distinct_errors_with_the_same_message() {
    let now = Instant::now();
    let mut store = Notifications::default();
    let error = |key: Option<&str>, code: &str| NoticeKey {
        key: key.map(str::to_owned),
        code: Some(code.into()),
        tone: Tone::Error,
        message: "Failed".into(),
    };
    assert!(store.push_keyed(error(None, "VALIDATION"), now).is_some());
    assert!(store.push_keyed(error(None, "INTERNAL"), now).is_some());
    assert!(
        store
            .push_keyed(error(Some("initial"), "INTERNAL"), now)
            .is_some()
    );
    assert!(store.push_keyed(error(None, "INTERNAL"), now).is_none());
}

#[test]
fn deduplicates_dismissed_errors_until_their_original_expiration() {
    let now = Instant::now();
    let mut notifications = Notifications::default();
    let id = notifications
        .push("Failed".into(), Tone::Error, now)
        .unwrap();
    assert!(notifications.dismiss(id));
    assert_eq!(
        notifications.push("Failed".into(), Tone::Error, now + Duration::from_secs(1)),
        None
    );
    assert_ne!(
        notifications.push("Failed".into(), Tone::Error, now + ERROR_DURATION),
        Some(id)
    );
    assert_eq!(notifications.messages.len(), 1);
}

#[test]
fn access_notices_track_new_peer_ids_not_repeated_snapshots() {
    let mut notifications = Notifications::default();
    assert_eq!(
        notifications
            .update_access([("alpha", "Workstation")])
            .as_deref(),
        Some("Workstation wants to connect")
    );
    assert_eq!(
        notifications.update_access([("alpha", "Workstation")]),
        None
    );
    assert_eq!(
        notifications
            .update_access([
                ("alpha", "Workstation"),
                ("beta", "Phone"),
                ("gamma", "Tablet")
            ])
            .as_deref(),
        Some("2 devices want to connect")
    );
    assert_eq!(notifications.update_access([]), None);
    assert_eq!(
        notifications.update_access([("beta", "Phone")]).as_deref(),
        Some("Phone wants to connect")
    );
    assert_eq!(Tone::AccessRequest.duration(), Duration::from_secs(12));
}

#[test]
fn information_uses_a_two_second_duration() {
    let now = Instant::now();
    let mut notifications = Notifications::default();
    assert!(
        notifications
            .push("Path copied".into(), Tone::Info, now)
            .is_some()
    );
    assert_eq!(
        notifications.push(
            "Path copied".into(),
            Tone::Info,
            now + Duration::from_secs(1)
        ),
        None
    );
    assert!(
        notifications
            .push(
                "Path copied".into(),
                Tone::Info,
                now + Duration::from_secs(2)
            )
            .is_some()
    );
}

#[test]
fn retains_only_the_last_four_visible_errors() {
    let mut notifications = Notifications::default();
    for id in 1..=6 {
        notifications.push(id.to_string(), Tone::Error, Instant::now());
    }
    let ids: Vec<_> = notifications
        .messages
        .iter()
        .map(|(id, _, _)| *id)
        .collect();
    assert_eq!(ids, [3, 4, 5, 6]);
}
