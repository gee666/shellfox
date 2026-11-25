//! The host process of the desktop app. The UI starts it and attaches over the
//! local API, so a UI crash leaves every shell running.
//!
//! `shellfox-cli --desktop-host UI_EXE N RELAUNCH_ARG... HOST_ARG...`
//!
//! stdout carries one line: the readiness marker or the startup error. stdin is
//! a pipe from the UI that started this host. The UI writes the quit byte before
//! it stops the host; EOF without it means the UI died, and the host starts it
//! again with the N relaunch arguments, unless this host is stopping. A
//! relaunched UI that exits with an error is started again, up to 3 times in
//! 5 minutes (relaunch.rs). Past that, an idle host stops, and one with shells
//! tells the user through an OS notification (notice.rs). If the login session
//! ended (logout, display gone) it stops instead, as a logout ended 0.9.x and
//! its shells. Later UIs attach on their own.
mod notice;
mod relaunch;

use anyhow::{ensure, Context, Result};
use shellfox_service::{
    desktop_launch::{DesktopLaunchReply, DesktopLaunchRequest},
    local::{LocalBootstrap, LocalClient},
};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

pub fn run(arguments: &[OsString]) -> i32 {
    match serve(arguments) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Shellfox desktop host ended: {error:#}");
            // The UI shows this line; a closed pipe only means it is gone.
            let _ = writeln!(std::io::stdout(), "{error:#}");
            1
        }
    }
}

struct Plan {
    ui: PathBuf,
    relaunch: Vec<OsString>,
    host: Vec<OsString>,
}

fn plan(arguments: &[OsString]) -> Result<Plan> {
    let mut arguments = arguments.iter().cloned();
    let ui = PathBuf::from(arguments.next().context("Desktop executable missing")?);
    ensure!(ui.is_absolute(), "Desktop executable must be absolute");
    let count: usize = arguments
        .next()
        .and_then(|count| count.into_string().ok())
        .and_then(|count| count.parse().ok())
        .context("Relaunch argument count missing")?;
    ensure!(count <= 64, "Too many relaunch arguments");
    let relaunch: Vec<_> = arguments.by_ref().take(count).collect();
    ensure!(relaunch.len() == count, "Relaunch arguments missing");
    Ok(Plan {
        ui,
        relaunch,
        host: arguments.collect(),
    })
}

fn serve(arguments: &[OsString]) -> Result<()> {
    let Plan {
        ui,
        relaunch,
        mut host,
    } = plan(arguments)?;
    host.extend(["host".into(), "serve".into()]);
    let args = crate::args::parse(host)?;
    let started = std::time::SystemTime::now();
    shellfox_service::crash_report::install(
        &args.data_dir,
        "host",
        env!("SHELLFOX_VERSION"),
        env!("SHELLFOX_BUILD_ID"),
    );
    ensure!(
        matches!(args.command, crate::args::Command::Serve),
        "Unexpected host command"
    );
    // The desktop host opens folders and links for its UI, unlike a headless host.
    let remote = args
        .application
        .remote_options(std::env::current_exe()?, true)?;
    shellfox_service::desktop_launch::set_host_build(env!("SHELLFOX_BUILD_ID"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let serving = Serving {
        data: args.data_dir.clone(),
        helper: args.helper.clone(),
        digest: args.helper_sha256,
    };
    let (ready, serving_now) = std::sync::mpsc::channel();
    watch_ui(ui, relaunch, serving, serving_now, started);
    LocalBootstrap::serve_foreground(
        &args.data_dir,
        &args.helper,
        args.helper_sha256,
        remote,
        &runtime,
        || {
            let mut stdout = std::io::stdout();
            let _ = writeln!(stdout, "{}", LocalBootstrap::DESKTOP_HOST_READY);
            let _ = stdout.flush();
            let _ = ready.send(());
        },
    )
}

struct Serving {
    data: PathBuf,
    helper: PathBuf,
    digest: [u8; 32],
}
impl Serving {
    /// A stopping host no longer routes desktop launches.
    fn routes_launches(&self) -> bool {
        self.ask(async |client| {
            let reply = client.request(&DesktopLaunchRequest::Discover {}).await?;
            anyhow::Ok(matches!(reply, DesktopLaunchReply::Route(_)))
        })
        .is_some_and(|routed| routed.unwrap_or(false))
    }
    /// No UI comes back by itself: an idle host stops, and the user learns
    /// about the shells of a busy one instead of them running unseen.
    fn without_ui(&self) {
        let terminals = self.ask(async |client| {
            match client
                .request(&shellfox_service::local::DesktopRequest::Status)
                .await?
            {
                shellfox_service::local::DesktopReply::Status(status) => {
                    Ok(status.active_terminals)
                }
                _ => anyhow::bail!("Host returned an invalid status response."),
            }
        });
        match terminals {
            Some(Ok(0)) => self.stop(),
            Some(Ok(terminals)) => notice::show(terminals),
            Some(Err(error)) => eprintln!("Shellfox host status failed: {error:#}"),
            None => eprintln!("Shellfox host status timed out"),
        }
    }
    /// The same orderly stop as `host stop`: the shells end with the host.
    fn stop(&self) {
        let stopped = self.ask(async |client| {
            client
                .execute(shellfox_service::local_api::LocalApiRequest::Stop {})
                .await?;
            anyhow::Ok(())
        });
        if let Some(Err(error)) = stopped {
            eprintln!("Shellfox host stop without a UI failed: {error:#}");
        }
    }
    /// None if the host did not answer in time.
    fn ask<T>(&self, work: impl AsyncFnOnce(&LocalClient) -> Result<T>) -> Option<Result<T>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        let asked = async {
            let client = LocalClient::connect(&self.data, &self.helper, self.digest).await?;
            let result = work(&client).await;
            let _ = client.disconnect().await;
            result
        };
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), asked)
                .await
                .ok()
        })
    }
}

/// What the host does once its UI ended without the quit byte.
#[derive(Debug, PartialEq, Eq)]
enum AfterUi {
    /// Logout or display loss: stop as 0.9.x did, shells included.
    Stop,
    Relaunch,
    /// The host is stopping anyway.
    Nothing,
}
fn after_ui(session_ended: bool, routes_launches: impl FnOnce() -> bool) -> AfterUi {
    if session_ended {
        AfterUi::Stop
    } else if routes_launches() {
        AfterUi::Relaunch
    } else {
        AfterUi::Nothing
    }
}

fn watch_ui(
    ui: PathBuf,
    relaunch: Vec<OsString>,
    serving: Serving,
    ready: std::sync::mpsc::Receiver<()>,
    started: std::time::SystemTime,
) {
    let watch = move || {
        // Decide only once the host serves: a UI that died during startup is
        // relaunched too, instead of leaving an idle host behind.
        if ready.recv().is_err() {
            return;
        }
        let mut stdin = std::io::stdin().lock();
        let mut byte = [0; 1];
        loop {
            match stdin.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) if byte[0] == LocalBootstrap::DESKTOP_HOST_QUIT => return,
                Ok(_) => {}
            }
        }
        let mut since = started;
        let mut relaunches = relaunch::Relaunches::default();
        loop {
            let after = after_ui(shellfox_service::desktop_session::ended(), || {
                serving.routes_launches()
            });
            eprintln!("Shellfox UI ended without quitting: {after:?}");
            match after {
                AfterUi::Stop => return serving.stop(),
                AfterUi::Nothing => return,
                AfterUi::Relaunch => {}
            }
            let report = shellfox_service::crash_report::ui_exit(since);
            let Some(wait) = relaunches.next(std::time::Instant::now()) else {
                eprintln!("Shellfox UI crashed too often; it is not relaunched");
                return serving.without_ui();
            };
            std::thread::sleep(wait);
            since = std::time::SystemTime::now();
            let mut command = Command::new(&ui);
            command.args(&relaunch).arg("--owned-host");
            if let Some(report) = report {
                command.arg("--crash-report").arg(report);
            }
            command
                .stdin(Stdio::null())
                // This host's stdout and stderr belong to the UI that died.
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                // A startup token belongs to the launch that already used it.
                .env_remove("DESKTOP_STARTUP_ID")
                .env_remove("XDG_ACTIVATION_TOKEN");
            let status = match command.spawn().and_then(|mut child| child.wait()) {
                Ok(status) => status,
                Err(error) => {
                    eprintln!("Shellfox UI relaunch failed: {error:#}");
                    return serving.without_ui();
                }
            };
            eprintln!("Shellfox relaunched UI ended: {status}");
            // A relaunched UI quits with its host and exits 0; anything else is a crash.
            if status.success() {
                if shellfox_service::desktop_session::ended() {
                    serving.stop();
                }
                return;
            }
        }
    };
    if let Err(error) = std::thread::Builder::new()
        .name("desktop-ui-watch".into())
        .spawn(watch)
    {
        eprintln!("Shellfox UI watch unavailable: {error:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn relaunch_and_host_arguments_are_split_by_count() {
        let ui = if cfg!(windows) {
            r"C:\Shellfox\shellfox.exe"
        } else {
            "/opt/shellfox/shellfox"
        };
        let plan = plan(&strings(&[
            ui,
            "2",
            "--desktop-dir",
            "/d",
            "--data-dir",
            "/h",
        ]))
        .unwrap();
        assert_eq!(plan.ui, PathBuf::from(ui));
        assert_eq!(plan.relaunch, strings(&["--desktop-dir", "/d"]));
        assert_eq!(plan.host, strings(&["--data-dir", "/h"]));
        assert!(super::plan(&strings(&[ui, "3", "--desktop-dir", "/d"])).is_err());
        assert!(super::plan(&strings(&["relative", "0"])).is_err());
        assert!(super::plan(&strings(&[ui])).is_err());
    }
    #[test]
    fn a_lost_ui_is_relaunched_unless_its_session_ended() {
        assert_eq!(
            after_ui(true, || panic!("an ended session never relaunches")),
            AfterUi::Stop
        );
        assert_eq!(after_ui(false, || true), AfterUi::Relaunch);
        assert_eq!(after_ui(false, || false), AfterUi::Nothing);
    }
}
