use super::*;
use crate::manager_args::{Filters, Listing};

fn stats() -> Value {
    json!({
        "sessions": [
            { "id": "a", "title": "Backend", "cwd": "/srv/api", "status": "running", "activityStatus": "running",
              "counts": { "agents": 1 }, "tabs": [{ "id": "t1", "title": "sh", "lifecycle": "open" }] },
            { "id": "b", "title": "Docs", "cwd": "/srv/docs", "status": "waiting", "activityStatus": "waiting",
              "counts": { "agents": 0 }, "tabs": [] },
        ],
        "tabs": [{ "tabId": "t1", "processes": [{ "pid": 7, "parentPid": 1, "name": "node", "domain": "local", "memoryBytes": 1048576, "elapsedSeconds": 61 }], "memoryBytes": 1048576, "elapsedSeconds": 3661 }],
    })
}

#[test]
fn filters_match_0_9_filter_control() {
    let names = |filters: Filters| {
        filter(&stats(), &filters)["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["title"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(Filters::default()).len(), 2);
    assert_eq!(
        names(Filters {
            running: true,
            ..Default::default()
        }),
        ["Backend"]
    );
    assert_eq!(
        names(Filters {
            idle: true,
            ..Default::default()
        }),
        ["Docs"]
    );
    assert_eq!(
        names(Filters {
            no_agents: true,
            ..Default::default()
        }),
        ["Docs"]
    );
    assert_eq!(
        names(Filters {
            path: Some("API".into()),
            ..Default::default()
        }),
        ["Backend"]
    );
    assert_eq!(
        names(Filters {
            pid: Some(7),
            ..Default::default()
        }),
        ["Backend"]
    );
    assert!(names(Filters {
        pid: Some(8),
        ..Default::default()
    })
    .is_empty());
}

#[test]
fn tables_and_trees_are_the_0_9_text() {
    let list = format(Listing::List, &stats(), Style(false), usize::MAX);
    assert!(list.starts_with("  NAME     ID  STATUS   AGENTS  PATH       TERMINALS\n  Backend  a   running  1       /srv/api   1\n"), "{list}");
    let processes = format(Listing::Processes, &stats(), Style(false), usize::MAX);
    assert!(processes.contains("  Backend  a  RSS 1.0 MiB  duration 1h 1m 1s\n    +- sh  t1  RSS 1.0 MiB  duration 1h 1m 1s\n      +- node  PID 7  PPID 1  local  RSS 1.0 MiB  duration 1m 1s"), "{processes}");
    assert!(processes.contains("  Docs  b  RSS n/a  duration n/a\n    No tabs."));
    assert_eq!(
        format(Listing::Stats, &json!({ "sessions": [] }), Style(false), 80),
        "  No matching sessions.\n"
    );
}
