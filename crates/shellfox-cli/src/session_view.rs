//! Session records, `session list|stats|processes` filters and their 0.9 tables
//! (src/main/control-cli.ts and ssh/presentation.ts). Records are JSON values so
//! local host records and a peer's v1 records go through the same rules.
use crate::{
    human_text::{escape, table, wrap, Style},
    manager_args::Filters,
};
use serde_json::{json, Value};
use shellfox_core::Session;
use shellfox_service::{
    activity::{session_facts, AgentState, TabFacts, TabLifecycle, TabStatus},
    NativeTabActivity,
};

/// The fallback record when the host has no v1 record for a session (an older
/// host, or a session the v1 shape omits): 0.9's live `status`, `activityStatus`
/// and `counts`, and each tab's `lifecycle`, `status` and `generation`.
/// The host reports whether a tab has agents, not how many, so `agents` is 0 or 1.
pub fn record(session: &Session, activity: &[NativeTabActivity]) -> Value {
    let mut value = serde_json::to_value(session).unwrap_or(Value::Null);
    let mut facts = Vec::with_capacity(session.tabs.len());
    if let Some(tabs) = value.get_mut("tabs").and_then(Value::as_array_mut) {
        for tab in tabs {
            let live = activity
                .iter()
                .find(|a| tab["id"].as_str() == Some(&a.tab.to_string()))
                .filter(|a| !a.closed);
            let error = live.is_some_and(|a| a.error.is_some() || a.dot == AgentState::Error);
            let status = match live {
                _ if error => TabStatus::Error,
                Some(a) if matches!(a.dot, AgentState::Running | AgentState::Busy) => {
                    TabStatus::Running
                }
                Some(_) => TabStatus::Waiting,
                None => TabStatus::Unknown,
            };
            let lifecycle = if live.is_some() {
                TabLifecycle::Open
            } else {
                TabLifecycle::Closed
            };
            let agents = u32::from(live.is_some_and(|a| a.has_agents));
            facts.push(TabFacts {
                lifecycle,
                status,
                agents,
                busy: false,
                error,
            });
            tab["lifecycle"] = json!(if live.is_some() { "open" } else { "closed" });
            tab["status"] = json!(status);
            tab["agents"] = json!(agents);
            tab["generation"] = json!(live.map(|a| a.generation));
        }
    }
    let summary = session_facts(&facts, session.settled_at.is_some(), false);
    value["status"] = json!(summary.status);
    value["activityStatus"] = json!(summary.activity_status);
    value["counts"] = json!(summary.counts);
    value
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}
fn contains(value: &str, term: &Option<String>) -> bool {
    term.as_ref()
        .is_none_or(|term| value.to_lowercase().contains(&term.to_lowercase()))
}

/// 0.9 `filterControl` over `{sessions, tabs}`.
pub fn filter(stats: &Value, filters: &Filters) -> Value {
    let tabs = stats["tabs"].as_array().cloned().unwrap_or_default();
    let processes = |tab: &str| {
        tabs.iter()
            .find(|t| t["tabId"] == tab)
            .and_then(|t| t["processes"].as_array().cloned())
            .unwrap_or_default()
    };
    let id = filters.id.map(|id| id.to_string());
    let tab = filters.tab.map(|id| id.to_string());
    let sessions: Vec<Value> = stats["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| {
            id.as_ref()
                .is_none_or(|id| text(s, "id").eq_ignore_ascii_case(id))
                && contains(text(s, "title"), &filters.name)
                && contains(text(s, "cwd"), &filters.path)
                && (!filters.running || s["activityStatus"] == "running")
                && (!filters.idle || (s["status"] != "settled" && s["activityStatus"] == "waiting"))
                && (!filters.no_agents || s["counts"]["agents"] == 0)
        })
        .map(|s| {
            let mut s = s.clone();
            if let Some(list) = s["tabs"].as_array_mut() {
                list.retain(|t| {
                    tab.as_ref()
                        .is_none_or(|tab| text(t, "id").eq_ignore_ascii_case(tab))
                        && filters.pid.is_none_or(|pid| {
                            processes(text(t, "id")).iter().any(|p| p["pid"] == pid)
                        })
                });
            }
            s
        })
        .filter(|s| {
            (filters.tab.is_none() && filters.pid.is_none())
                || s["tabs"].as_array().is_some_and(|t| !t.is_empty())
        })
        .collect();
    let kept: Vec<String> = sessions
        .iter()
        .flat_map(|s| s["tabs"].as_array().cloned().unwrap_or_default())
        .map(|t| text(&t, "id").to_owned())
        .collect();
    let tabs: Vec<Value> = tabs
        .into_iter()
        .filter(|t| kept.iter().any(|id| t["tabId"] == id.as_str()))
        .map(|mut t| {
            if let (Some(pid), Some(list)) = (filters.pid, t["processes"].as_array_mut()) {
                list.retain(|p| p["pid"] == pid);
            }
            t
        })
        .collect();
    json!({ "sessions": sessions, "tabs": tabs })
}

fn rss(value: &Value) -> String {
    value.as_f64().filter(|bytes| *bytes >= 0.0).map_or_else(
        || "n/a".into(),
        |bytes| format!("{:.1} MiB", bytes / 1_048_576.0),
    )
}
fn duration(value: &Value) -> String {
    let Some(total) = value.as_f64().filter(|s| *s >= 0.0).map(|s| s as u64) else {
        return "n/a".into();
    };
    let (hours, minutes) = (total / 3600, total / 60 % 60);
    let mut text = String::new();
    if hours > 0 {
        text.push_str(&format!("{hours}h "));
    }
    if hours > 0 || minutes > 0 {
        text.push_str(&format!("{minutes}m "));
    }
    format!("{text}{}s", total % 60)
}
fn aggregate(stats: &[Option<&Value>], key: &str, sum: bool) -> Value {
    if stats.is_empty() || stats.iter().any(|s| s.is_none_or(|s| !s[key].is_number())) {
        return Value::Null;
    }
    let values = stats.iter().filter_map(|s| s.and_then(|s| s[key].as_f64()));
    json!(if sum {
        values.sum::<f64>()
    } else {
        values.fold(0.0, f64::max)
    })
}

/// 0.9 `formatControl`: a session table for `list`, a tree for stats and processes.
pub fn format(
    listing: crate::manager_args::Listing,
    stats: &Value,
    style: Style,
    width: usize,
) -> String {
    let sessions = stats["sessions"].as_array().cloned().unwrap_or_default();
    if sessions.is_empty() {
        return "  No matching sessions.\n".into();
    }
    if listing == crate::manager_args::Listing::List {
        let headers = ["NAME", "ID", "STATUS", "AGENTS", "PATH", "TERMINALS"].map(str::to_owned);
        let rows: Vec<Vec<String>> = sessions
            .iter()
            .map(|s| {
                vec![
                    escape(text(s, "title")),
                    escape(text(s, "id")),
                    text(s, "status").to_owned(),
                    s["counts"]["agents"].as_u64().unwrap_or(0).to_string(),
                    escape(text(s, "cwd")),
                    s["tabs"].as_array().map_or(0, Vec::len).to_string(),
                ]
            })
            .collect();
        let width = width.saturating_sub(2).max(1);
        return table(&headers, &rows, style, width, &|v| v.to_owned())
            .split('\n')
            .map(|line| format!("  {line}\n"))
            .collect();
    }
    let all: Vec<Value> = stats["tabs"].as_array().cloned().unwrap_or_default();
    let stat = |id: &str| all.iter().find(|t| t["tabId"] == id);
    let mut lines = Vec::new();
    for session in &sessions {
        let tabs: Vec<Value> = session["tabs"].as_array().cloned().unwrap_or_default();
        let measured: Vec<Option<&Value>> = tabs
            .iter()
            .filter(|t| t["lifecycle"] != "closed" || stat(text(t, "id")).is_some())
            .map(|t| stat(text(t, "id")))
            .collect();
        lines.push(format!(
            "  {}  {}  RSS {}  duration {}",
            style.accent(&escape(text(session, "title"))),
            escape(text(session, "id")),
            rss(&aggregate(&measured, "memoryBytes", true)),
            duration(&aggregate(&measured, "elapsedSeconds", false))
        ));
        if tabs.is_empty() {
            lines.push("    No tabs.".into());
        }
        for tab in &tabs {
            let found = stat(text(tab, "id"));
            let none = Value::Null;
            let measure = |key| found.map_or(&none, |s| &s[key]);
            lines.push(format!(
                "    +- {}  {}  RSS {}  duration {}",
                escape(text(tab, "title")),
                escape(text(tab, "id")),
                rss(measure("memoryBytes")),
                duration(measure("elapsedSeconds"))
            ));
            let reason = found
                .and_then(|s| s["reason"].as_str())
                .or_else(|| tab["monitoringReason"].as_str());
            if let Some(reason) = reason {
                lines.push(format!("       {}", escape(reason)));
            }
            if listing == crate::manager_args::Listing::Processes {
                let processes = found
                    .and_then(|s| s["processes"].as_array())
                    .cloned()
                    .unwrap_or_default();
                for p in &processes {
                    lines.push(format!(
                        "      +- {}  PID {}  PPID {}  {}  RSS {}  duration {}",
                        escape(text(p, "name")),
                        p["pid"],
                        p["parentPid"],
                        escape(text(p, "domain")),
                        rss(&p["memoryBytes"]),
                        duration(&p["elapsedSeconds"])
                    ));
                }
                if processes.is_empty() {
                    lines.push(if found.is_some() {
                        "       No observed processes.".into()
                    } else {
                        "       Processes n/a.".into()
                    });
                }
            }
        }
    }
    format!("{}\n", wrap(&lines.join("\n"), width))
}

#[cfg(test)]
#[path = "session_view_tests.rs"]
mod tests;
