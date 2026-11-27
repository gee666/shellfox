//! Opt-in checks against a real WSL distribution: a program that matches an
//! agent rule in a WSL tab, or in `wsl` started from a PowerShell tab, shows
//! as an agent in the v1 records and pulses on the desktop while it prints.
//! Input goes through the desktop's own actions.
use crate::activity::AgentState;
use crate::native::{NativeAction, NativeCommand, NativeInputContext, NativeResponse};
use crate::{Attachment, Service};
use anyhow::{Context, Result};
use shellfox_core::{CreateSession, ProfileId, SessionId, TabId};
use shellfox_ghostty_vt::key::{KeyEvent, KeyModifiers, PhysicalKey};
use shellfox_protocol::{CancellationToken, Status, TabStatus};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// The tab's v1 status and agent count, and its desktop dot.
fn state(service: &mut Service, tab: TabId) -> Result<(TabStatus, u64, Option<AgentState>)> {
    service.pump_activity();
    let records = service.session_records()?;
    let session = &records[0];
    let record = session
        .tabs
        .iter()
        .find(|record| record.id == tab)
        .context("tab record")?;
    // The session record agrees with its only tab.
    anyhow::ensure!(
        session.counts.agents == record.agents
            && matches!(session.status, Status::Running)
                == matches!(record.status, TabStatus::Running),
        "session {:?} with {} agents disagrees with its tab",
        session.status,
        session.counts.agents
    );
    let dot = service
        .desktop_tab_activity()
        .into_iter()
        .find(|activity| activity.tab == tab)
        .map(|activity| activity.dot);
    Ok((record.status, record.agents, dot))
}

/// Waits until the tab shows `agents` and the desktop dot `dot`.
fn wait_for(service: &mut Service, tab: TabId, agents: u64, dot: AgentState) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (status, seen, shown) = state(service, tab)?;
        let running = matches!(status, TabStatus::Running);
        if seen == agents && running == (agents > 0) && shown == Some(dot) {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "tab stayed {status:?} with {seen} agents and dot {shown:?}, wanted {agents} and {dot:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Tab {
    id: TabId,
    control: Attachment,
    settled_at: Option<String>,
}
impl Tab {
    fn send(&self, service: &mut Service, command: NativeCommand) -> Result<()> {
        let input = NativeInputContext {
            captured_settlement: shellfox_host::CapturedSettlement::Captured(
                self.settled_at.clone(),
            ),
            user_initiated: true,
            hint: true,
        };
        service.execute_native(
            NativeAction::Terminal {
                tab: self.id,
                attachment: self.control.clone(),
                command,
                input: Some(input),
            },
            1,
        )?;
        Ok(())
    }
    /// Types `line` and Enter, as the desktop sends them.
    fn run(&self, service: &mut Service, line: String) -> Result<()> {
        self.send(service, NativeCommand::Paste(line))?;
        let enter = KeyEvent::press(PhysicalKey::Enter, KeyModifiers::NONE);
        self.send(service, NativeCommand::Key(enter))
    }
    fn interrupt(&self, service: &mut Service) -> Result<()> {
        let ctrl_c = KeyEvent {
            text: String::new(),
            ..KeyEvent::press(PhysicalKey::KeyC, KeyModifiers::CONTROL)
        };
        self.send(service, NativeCommand::Key(ctrl_c))
    }
}

/// Runs `check` on a fresh tab of `profile` in a scratch host.
fn with_tab(
    profile: impl FnOnce(&str) -> String,
    check: impl FnOnce(&mut Service, &Tab, &str) -> Result<()>,
) -> Result<()> {
    let distro = std::env::var("SHELLFOX_WSL_TEST_DISTRO").context("WSL distribution")?;
    let supervisor = PathBuf::from(
        std::env::var_os("SHELLFOX_NATIVE_TEST_SUPERVISOR").context("explicit test artifact")?,
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root)?;
    let data = tempfile::Builder::new()
        .prefix("wsl-agent-")
        .tempdir_in(root)?;
    let mut service = Service::open_owned(data.path(), &supervisor)?;
    let (lost, wake) = (service.ownership_lost.clone(), service.wake_on_exit());
    service.catalog.wsl.start(&lost, wake);
    service
        .catalog
        .wsl
        .cache
        .wait(&CancellationToken::new())
        .context("WSL profiles")?;
    // As the desktop creates one: v1 records need its creation shell.
    let snapshot = service.create_application_session(CreateSession {
        request_id: SessionId::new(),
        title: None,
        cwd: data.path().to_string_lossy().into_owned(),
        profile_id: ProfileId::new(profile(&distro))?,
    })?;
    let id = snapshot.sessions[0].tabs[0].id;
    let attach = NativeAction::Attach {
        tab: id,
        takeover: false,
    };
    let result = match service.execute_native(attach, 1) {
        Ok(NativeResponse::Attached(control)) => {
            let settled_at = snapshot.sessions[0].settled_at.clone();
            check(
                &mut service,
                &Tab {
                    id,
                    control,
                    settled_at,
                },
                &distro,
            )
        }
        Ok(_) => Err(anyhow::anyhow!("no control attachment")),
        Err(error) => Err(error),
    };
    service
        .shutdown()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    result
}

/// A copy of bash named like an agent matches the default Claude rule only
/// if the tab's marker reached the guest. It prints for three seconds, then
/// waits quietly.
fn run_agent(service: &mut Service, tab: &Tab) -> Result<()> {
    let agent = format!("/tmp/shellfox-agent-{}", tab.id);
    let script =
        "for i in $(seq 30); do echo working $i; sleep 0.1; done; while :; do sleep 1; done";
    tab.run(
        service,
        format!("mkdir -p {agent} && cp /bin/bash {agent}/claude && {agent}/claude -c '{script}'"),
    )?;
    wait_for(service, tab.id, 1, AgentState::Busy)?;
    // Quiet output ends the working pulse; the agent stays.
    wait_for(service, tab.id, 1, AgentState::Running)?;
    tab.interrupt(service)?;
    wait_for(service, tab.id, 0, AgentState::Shell)?;
    // Ctrl+C ends the whole command line, so this cleans up separately.
    tab.run(service, format!("rm -r {agent}"))
}

#[test]
#[ignore = "needs SHELLFOX_WSL_TEST_DISTRO and a supervisor in SHELLFOX_NATIVE_TEST_SUPERVISOR"]
fn a_wsl_tab_running_an_agent_reports_it_and_its_work() -> Result<()> {
    with_tab(
        |distro| format!("wsl:{distro}"),
        |service, tab, _| run_agent(service, tab),
    )
}

#[test]
#[ignore = "needs SHELLFOX_WSL_TEST_DISTRO and a supervisor in SHELLFOX_NATIVE_TEST_SUPERVISOR"]
fn wsl_started_in_a_powershell_tab_reports_its_agents() -> Result<()> {
    with_tab(
        |_| "windows-powershell".into(),
        |service, tab, distro| {
            tab.run(service, format!("wsl.exe -d {distro}"))?;
            std::thread::sleep(Duration::from_secs(3));
            run_agent(service, tab)
        },
    )
}
