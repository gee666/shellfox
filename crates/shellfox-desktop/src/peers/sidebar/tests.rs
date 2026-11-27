use super::*;

fn session(n: u32, title: &str, created_at: &str, pinned_at: Option<&str>) -> SessionDto {
    serde_json::from_value(serde_json::json!({
        "id": format!("00000000-0000-4000-8000-{n:012}"),
        "title": title,
        "cwd": "/demo",
        "adapterId": "embedded-pty",
        "shellId": "pwsh",
        "createdAt": created_at,
        "updatedAt": created_at,
        "settledAt": null,
        "pinnedAt": pinned_at,
        "status": "waiting",
        "activityStatus": "waiting",
        "counts": { "running": 0, "waiting": 0, "unknown": 0, "error": 0, "closed": 0, "agents": 0 },
        "tabs": [],
        "canFocus": false,
        "canAddTab": true,
        "controlReason": null,
        "error": null,
    }))
    .unwrap()
}

#[test]
fn peer_rows_pin_first_then_dot_then_newest_then_name() {
    let sessions = [
        session(1, "old", "2026-01-01T00:00:00.000Z", None),
        session(2, "new", "2026-02-01T00:00:00.000Z", None),
        session(3, "Shell 10", "2026-01-15T00:00:00.000Z", None),
        session(4, "shell 2", "2026-01-15T00:00:00.000Z", None),
        session(5, "error", "2025-01-01T00:00:00.000Z", None),
        session(
            6,
            "late pin",
            "2025-01-01T00:00:00.000Z",
            Some("2026-03-02T00:00:00.000Z"),
        ),
        session(
            7,
            "early pin",
            "2025-01-01T00:00:00.000Z",
            Some("2026-03-01T00:00:00.000Z"),
        ),
    ];
    let dot = |title: &str| match title {
        "error" | "late pin" => AgentState::Error,
        "early pin" => AgentState::Settled,
        _ => AgentState::Shell,
    };
    let mut rows: Vec<_> = sessions.iter().map(|s| (s, dot(&s.title))).collect();
    sort_rows(&mut rows);
    let titles: Vec<_> = rows.iter().map(|(s, _)| s.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "late pin",
            "early pin",
            "error",
            "new",
            "shell 2",
            "Shell 10",
            "old"
        ]
    );
}
