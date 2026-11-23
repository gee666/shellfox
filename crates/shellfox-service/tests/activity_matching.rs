use shellfox_core::{default_process_rules, Generation, SessionId, SettingsProcessRule, TabId};
use shellfox_runtime::Identity;
use shellfox_service::activity::*;

struct Once(Option<OwnedProcessSnapshot>);
impl ProcessInspector for Once {
    fn inspect(&mut self, _: Identity) -> Result<OwnedProcessSnapshot, InspectionFailure> {
        self.0.take().ok_or(InspectionFailure::Unavailable)
    }
}
fn row(executable: &str, argv: Option<Vec<String>>) -> OwnedProcess {
    OwnedProcess {
        identity: ProcessIdentity {
            domain: "owned".into(),
            pid: 11,
            birth: "exact:11".into(),
        },
        is_root: false,
        accessible: true,
        executable: Some(executable.into()),
        process_name: None,
        argv,
    }
}
fn observe(child: OwnedProcess, rules: &[SettingsProcessRule], windows: bool) -> Observation {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let shell = if windows {
        "C:\\PowerShell\\pwsh.exe"
    } else {
        "/bin/bash"
    };
    let mut root = row(shell, None);
    root.is_root = true;
    root.identity.pid = 10;
    root.identity.birth = "exact:10".into();
    let snapshot = OwnedProcessSnapshot {
        identity,
        platform: if windows {
            ProcessPlatform::Windows
        } else {
            ProcessPlatform::Unix
        },
        root: RootState::Alive,
        complete: true,
        processes: vec![root, child],
    };
    let mut inspector = Once(Some(snapshot));
    let mut monitor = ActivityMonitor::new();
    monitor.watch(identity, shell.into()).unwrap();
    monitor
        .poll(&mut inspector, rules, 0, 1)
        .remove(0)
        .observation
}
fn args(values: &[&str]) -> Option<Vec<String>> {
    Some(values.iter().map(|value| (*value).into()).collect())
}
fn script_rule(executable: &str) -> SettingsProcessRule {
    SettingsProcessRule {
        id: "00000000-0000-4000-8000-000000000001".into(),
        label: "Agent".into(),
        enabled: true,
        executable_basenames: vec![executable.into()],
        executable_paths: vec![],
        script_path_suffixes: vec!["agent/cli.js".into()],
        process_names: None,
    }
}

#[test]
fn script_slots_never_search_arbitrary_prompt_eval_or_import_arguments() {
    let cases: &[(&[&str], u32, Health)] = &[
        (&["node", "/pkg/agent/cli.js"], 1, Health::Healthy),
        (&["node", "--", "/pkg/agent/cli.js"], 1, Health::Healthy),
        (
            &["node", "--require", "/pkg/agent/cli.js", "/pkg/other.js"],
            0,
            Health::Healthy,
        ),
        (
            &["node", "--title", "/pkg/agent/cli.js", "/pkg/other.js"],
            0,
            Health::Healthy,
        ),
        (&["node", "--eval", "/pkg/agent/cli.js"], 0, Health::Healthy),
        (
            &["node", "-phello", "/pkg/agent/cli.js"],
            0,
            Health::Healthy,
        ),
        (
            &["node", "--unknown-option", "/pkg/agent/cli.js"],
            0,
            Health::Unknown,
        ),
        (&["node", "-r"], 0, Health::Unknown),
        (
            &["node", "/pkg/notagent/cli.js", "agent/cli.js"],
            0,
            Health::Healthy,
        ),
        (
            &[
                "node",
                "--inspect=127.0.0.1",
                "--enable-source-maps",
                "/pkg/agent/cli.js",
            ],
            1,
            Health::Healthy,
        ),
    ];
    for (argv, agents, health) in cases {
        let result = observe(
            row("/usr/bin/node", args(argv)),
            &[script_rule("node")],
            false,
        );
        assert_eq!(
            (result.agents, result.health),
            (*agents, *health),
            "{argv:?}"
        );
    }
}

#[test]
fn bun_script_paths_and_powershell_file_mode_match_without_guessing_wrappers() {
    for (executable, argv, agents, health) in [
        (
            "bun",
            vec!["bun", "run", "/pkg/agent/cli.js"],
            1,
            Health::Healthy,
        ),
        ("bun", vec!["bun", "run", "build"], 0, Health::Healthy),
        (
            "bun",
            vec!["bun", "x", "/pkg/agent/cli.js"],
            0,
            Health::Healthy,
        ),
        (
            "pwsh",
            vec![
                "pwsh",
                "-NoLogo",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "/pkg/agent/cli.js",
            ],
            1,
            Health::Healthy,
        ),
        (
            "pwsh",
            vec!["pwsh", "-Command", "echo", "-File", "/pkg/agent/cli.js"],
            0,
            Health::Healthy,
        ),
        (
            "pwsh",
            vec!["pwsh", "-WorkingDirectory", "-File", "/pkg/agent/cli.js"],
            0,
            Health::Unknown,
        ),
    ] {
        let result = observe(
            row(&format!("/usr/bin/{executable}"), args(&argv)),
            &[script_rule(executable)],
            false,
        );
        assert_eq!((result.agents, result.health), (agents, health));
    }
}

#[test]
fn native_names_comm_and_rewritten_argv0_use_toggles_before_script_fallbacks() {
    for evidence in 0..3 {
        let rules = default_process_rules();
        let mut child = row("/usr/bin/node", args(&["node", "/unrelated.js"]));
        match evidence {
            0 => {
                child.executable = Some("/opt/pi".into());
                child.argv = None;
            }
            1 => child.process_name = Some("pi".into()),
            _ => child.argv = args(&["pi"]),
        }
        assert_eq!(observe(child, &rules, false).agents, 1);
    }
    let mut rules = default_process_rules();
    rules[0].enabled = false;
    let child = row(
        "/usr/bin/node",
        args(&["pi", "/pkg/@anthropic-ai/claude-code/cli.js"]),
    );
    assert_eq!(observe(child, &rules, false).agents, 0);
    let child = row(
        "/usr/bin/node",
        args(&["pi", "/pkg/@anthropic-ai/claude-code/cli.js"]),
    );
    rules[0].enabled = true;
    rules[1].enabled = false;
    assert_eq!(observe(child, &rules, false).agents, 1);
}

#[test]
fn every_supported_agent_projects_the_same_idle_status() {
    for windows in [false, true] {
        let node = if windows {
            r"C:\Tools\node.exe"
        } else {
            "/usr/bin/node"
        };
        let custom = if windows { "agent.exe" } else { "agent" };
        let mut rules = default_process_rules();
        let mut rule = script_rule(custom);
        rule.script_path_suffixes.clear();
        rules.push(rule);
        for (name, script) in [
            ("claude", "/pkg/@anthropic-ai/claude-code/cli.js"),
            ("pi", "/pkg/@earendil-works/pi-coding-agent/dist/cli.js"),
            ("codex", "/pkg/@openai/codex/bin/codex.js"),
            ("agent", ""),
        ] {
            let binary = if windows {
                format!(r"C:\Tools\{name}.exe")
            } else {
                format!("/opt/{name}")
            };
            let processes = std::iter::once(row(&binary, None))
                .chain((!script.is_empty()).then(|| row(node, args(&[node, script]))));
            for process in processes {
                let observation = observe(process, &rules, windows);
                assert_eq!(observation.agents, 1, "{name}");
                let lifecycle = TabLifecycle::Open;
                let status = tab_status(lifecycle, false, Some(&observation));
                assert_eq!(status, TabStatus::Running, "{name}");
                let tab = TabFacts {
                    lifecycle,
                    status,
                    agents: observation.agents,
                    busy: false,
                    error: false,
                };
                assert_eq!(tab_agent_state(&tab), AgentState::Running, "{name}");
                assert_eq!(
                    session_facts(&[tab], false, false).agent_state,
                    AgentState::Running,
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn custom_paths_override_names_and_windows_paths_are_case_insensitive() {
    let mut rule = script_rule("node.exe");
    rule.script_path_suffixes.clear();
    rule.executable_paths = vec!["C:\\Tools\\Agent.exe".into()];
    rule.process_names = Some(vec!["pi.exe".into()]);
    assert_eq!(
        observe(
            row("C:\\Other\\pi.exe", None),
            std::slice::from_ref(&rule),
            true
        )
        .agents,
        0
    );
    assert_eq!(
        observe(row("c:/TOOLS/x/../AGENT.EXE", None), &[rule], true).agents,
        1
    );
    let mut rule = script_rule("node");
    rule.executable_paths = vec!["/opt/Agent".into()];
    rule.script_path_suffixes.clear();
    assert_eq!(
        observe(row("/opt/agent", None), std::slice::from_ref(&rule), false).agents,
        0
    );
    assert_eq!(observe(row("/opt/Agent", None), &[rule], false).agents, 1);
}

#[test]
fn missing_argv_is_unknown_only_for_relevant_script_rules_and_near_names_do_not_match() {
    let rule = script_rule("node");
    assert_eq!(
        observe(
            row("/usr/bin/node", None),
            std::slice::from_ref(&rule),
            false
        )
        .health,
        Health::Unknown
    );
    let unrelated = observe(row("/usr/bin/sleep", None), &[rule], false);
    assert_eq!((unrelated.agents, unrelated.health), (0, Health::Healthy));
    for name in ["notpi", "pi-coding-agent", "pi --prompt text"] {
        let mut child = row("/usr/bin/node", args(&["node", "/unrelated.js", "pi"]));
        child.process_name = Some(name.into());
        assert_eq!(observe(child, &default_process_rules(), false).agents, 0);
    }
}
