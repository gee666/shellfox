use serde_json::{json, Value};
use shellfox_core as core;
use shellfox_protocol as wire;
use shellfox_remote::adapter::*;
use std::collections::HashSet;

pub const SESSION: &str = "11111111-1111-4111-8111-111111111111";
pub const TAB: &str = "22222222-2222-4222-8222-222222222222";
pub const GENERATION: &str = "33333333-3333-4333-8333-333333333333";
pub const TIME: &str = "2026-07-07T12:00:00.000Z";

pub fn snapshot(platform: &str) -> core::Snapshot {
    let cwd = if platform == "win32" {
        "C:\\work"
    } else {
        "/work"
    };
    serde_json::from_value(json!({
        "revision": 7,
        "sessions": [{
            "id": SESSION, "title": "Work", "cwd": cwd,
            "createdAt": TIME, "updatedAt": TIME, "settledAt": null, "pinnedAt": null,
            "env": [{"name": "SAVED", "value": "saved, not inherited"}],
            "tabs": [{
                "id": TAB, "sessionId": SESSION, "title": "Shell", "cwd": cwd,
                "ordinal": 0, "profileId": "login-shell", "state": "pending"
            }]
        }],
        "preferences": {
            "fontFamily": "monospace", "fontSize": 14,
            "accentColor": "#123456", "backgroundColor": "#654321",
            "defaultProfileId": "login-shell"
        },
        "selection": { "sessionId": SESSION, "tabId": TAB }
    }))
    .unwrap()
}

pub fn facts(platform: &str) -> ManagerFacts {
    let mut facts = ManagerFacts {
        settings: Some(SettingsFacts {
            adapter_id: wire::AdapterId::EmbeddedPty,
            shell_id: wire::ShellId::LoginShell,
            shell_executable: None,
            process_rules: vec![serde_json::from_value(json!({
                "id": "custom-agent-1", "label": "Agent", "enabled": true,
                "executableBasenames": ["agent"], "executablePaths": [],
                "scriptPathSuffixes": ["tools/agent.js"]
            })).unwrap()],
            history_page_size: 17,
            session_settle_hours: 48.0,
            session_delete_hours: 1440.0,
            auto_update: false,
            terminal_env: vec![core::EnvVar { name: "GLOBAL".into(), value: "kept".into() }],
        }),
        probe: Some(serde_json::from_value(json!({
            "platform": platform, "arch": "arm64", "adapterId": "embedded-pty",
            "available": false, "terminalVersion": null,
            "capabilities": {
                "createWindow": false, "addTab": false, "focusWindow": false,
                "activateTab": false, "splitPane": false, "attachExisting": false,
                "closeTerminal": false, "commandExitStatus": false,
                "processTracking": false, "explorerContextMenu": false,
                "embeddedTerminal": true, "terminalLifetime": "app-owned", "shellSurvival": false
            },
            "shells": [], "reasons": ["Host has no terminal capability"]
        })).unwrap()),
        explorer: Some(wire::ExplorerIntegration {
            supported: false, installed: false, folder_item_installed: false,
            background_installed: false, reason: Some("Not available on this host".into()),
        }),
        cli: Some(wire::CliIntegration {
            supported: false, installed: false, command: "shellfox".into(),
            reason: Some("Not installed".into()),
        }),
        sessions: Default::default(),
        omitted_sessions: HashSet::new(),
    };
    facts
        .sessions
        .insert(SESSION.parse().unwrap(), session_facts());
    facts
}

pub fn session_facts() -> SessionFacts {
    SessionFacts {
        adapter_id: wire::AdapterId::EmbeddedPty,
        shell_id: wire::ShellId::LoginShell,
        window: Some(wire::SessionWindow {
            state: wire::WindowState::Unsupported,
            can_reopen: false,
            can_register: false,
            reason: "No external window integration".into(),
        }),
        can_focus: false,
        can_add_tab: false,
        control_reason: Some("No local terminal capability".into()),
        error: None,
        terminal_lifetime: Some(wire::TerminalLifetime::AppOwned),
        shell_survival: Some(false),
        tabs: [(
            TAB.parse().unwrap(),
            TabFacts {
                created_at: "2026-07-07T12:00:01.000Z".into(),
                lifecycle: wire::TabLifecycle::Launching,
                observation: ObservationFacts::Unavailable {
                    reason: "Process monitor unavailable".into(),
                },
                error: None,
                generation: None,
                terminal_kind: Some(wire::TerminalKind::Embedded),
                exit_code: Some(None),
                process_title: None,
            },
        )]
        .into(),
    }
}

pub fn attachment() -> AttachmentFacts {
    AttachmentFacts {
        generation: GENERATION.parse().unwrap(),
        control_epoch: 3,
        delivery: AttachmentDelivery::Owned(ReplayFacts {
            first_sequence: 1,
            last_sequence: 2,
            chunks: vec![
                AnsiChunk {
                    sequence: 1,
                    data: "\u{1b}[31mhello".into(),
                },
                AnsiChunk {
                    sequence: 2,
                    data: "世界\r\n".into(),
                },
            ],
            truncated: false,
            recovery: RecoveryCapability::Replay,
            state: wire::ScreenState::Open,
            exit_code: None,
            cols: 80,
            rows: 24,
            lifetime: wire::TerminalLifetime::AppOwned,
        }),
    }
}

pub fn events() -> Vec<TerminalEventFacts> {
    let generation = GENERATION.parse().unwrap();
    vec![
        TerminalEventFacts::Settlement { generation },
        TerminalEventFacts::Activity { busy: false },
        TerminalEventFacts::Data {
            generation,
            control_epoch: 3,
            sequence: 2,
            data: "世界\r\n".into(),
        },
        TerminalEventFacts::Exit {
            generation,
            control_epoch: 3,
            exit_code: None,
            signal: None,
            last_sequence: 2,
        },
        TerminalEventFacts::Error {
            generation,
            control_epoch: 3,
            error: wire::AppError {
                code: wire::ErrorCode::MonitorUnavailable,
                message: "Monitoring unavailable".into(),
                retryable: false,
            },
        },
        TerminalEventFacts::Control {
            generation,
            control_epoch: 3,
            control: wire::Control::Blocked,
        },
    ]
}

pub fn status_sessions() -> Vec<wire::SessionDto> {
    let base = snapshot("linux");
    let original = &base.sessions[0];
    let original_facts = session_facts();
    let mut sessions = Vec::new();
    // All pairs cover live/closed/error precedence and closed-count handling.
    for first in 0..5 {
        for second in 0..5 {
            let mut session = original.clone();
            session.tabs.clear();
            let mut facts = original_facts.clone();
            facts.tabs.clear();
            for (ordinal, state) in [first, second].into_iter().enumerate() {
                let mut tab = original.tabs[0].clone();
                tab.id = format!("44444444-4444-4444-8444-{ordinal:012}")
                    .parse()
                    .unwrap();
                tab.ordinal = ordinal.try_into().unwrap();
                let mut tab_facts = original_facts.tabs.values().next().unwrap().clone();
                tab_facts.lifecycle = wire::TabLifecycle::Open;
                tab_facts.observation = match state {
                    0 => ObservationFacts::Unavailable {
                        reason: "Monitor unavailable".into(),
                    },
                    1 => ObservationFacts::Alive {
                        agents: 0,
                        healthy: true,
                        reason: None,
                    },
                    2 => ObservationFacts::Alive {
                        agents: 2,
                        healthy: true,
                        reason: None,
                    },
                    _ => {
                        tab_facts.lifecycle = wire::TabLifecycle::Closed;
                        ObservationFacts::Exited
                    }
                };
                if state == 4 {
                    tab_facts.error = Some(wire::AppError {
                        code: wire::ErrorCode::LaunchFailed,
                        message: "Failed".into(),
                        retryable: false,
                    });
                }
                facts.tabs.insert(tab.id, tab_facts);
                session.tabs.push(tab);
            }
            if first == 0 {
                session.settled_at = Some(TIME.into());
            }
            sessions.push(session_dto(&session, &facts).unwrap());
        }
    }
    sessions
}

pub fn fixture_document() -> Value {
    let platforms = ["win32", "linux", "darwin", "android", "ios", "headless"];
    let snapshots: Vec<_> = platforms
        .iter()
        .map(|platform| manager_snapshot(&snapshot(platform), &facts(platform)).unwrap())
        .collect();
    let metadata = snapshot("linux");
    let tab_id = TAB.parse().unwrap();
    let mut attachment_facts = attachment();
    let mut attachments =
        vec![terminal_attachment(&metadata, tab_id, Some(&attachment_facts)).unwrap()];
    if let AttachmentDelivery::Owned(replay) = &mut attachment_facts.delivery {
        replay.recovery = RecoveryCapability::AnsiV1;
        replay.state = wire::ScreenState::Closed;
        replay.exit_code = Some(0);
        replay.truncated = true;
    }
    attachments.push(terminal_attachment(&metadata, tab_id, Some(&attachment_facts)).unwrap());
    attachment_facts.delivery = AttachmentDelivery::Blocked;
    attachments.push(terminal_attachment(&metadata, tab_id, Some(&attachment_facts)).unwrap());
    let mut events: Vec<_> = events()
        .iter()
        .map(|facts| terminal_event(&metadata, tab_id, Some(facts)).unwrap())
        .collect();
    for reason in [core::ChangeReason::Sessions, core::ChangeReason::Settings] {
        events.push(
            changed_event(&core::ChangedEvent {
                revision: 8,
                reason,
                select_session_id: Some(SESSION.parse().unwrap()),
            })
            .unwrap(),
        );
    }
    let persisted: Vec<_> = [
        None,
        Some(None),
        Some(Some("opaque-imported-profile".into())),
    ]
    .into_iter()
    .map(|profile| {
        let settings = core::Settings {
            terminal_profile_id: profile,
            ..Default::default()
        };
        persisted_settings(&settings).unwrap()
    })
    .collect();
    json!({ "persistedSettings": persisted, "settings": snapshots.iter().map(|snapshot| &snapshot.settings).collect::<Vec<_>>(),
        "snapshots": snapshots, "attachments": attachments, "events": events,
        "statusSessions": status_sessions() })
}
