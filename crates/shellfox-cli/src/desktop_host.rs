//! The host process of the desktop app. The UI starts it and attaches over the
//! local API, so a UI crash leaves every shell running.
//!
//! `shellfox-cli --desktop-host UI_EXE N RELAUNCH_ARG... HOST_ARG...`
//!
//! stdout carries one line: the readiness marker or the startup error. stdin is
//! a pipe from the UI that started this host. The UI writes the quit byte before
//! it stops the host; EOF without it means the UI died, and the host starts it
//! again once with the N relaunch arguments, unless this host is stopping.
//! Later UIs attach on their own.
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
    Ok(Plan { ui, relaunch, host: arguments.collect() })
}

fn serve(arguments: &[OsString]) -> Result<()> {
    let Plan { ui, relaunch, mut host } = plan(arguments)?;
    host.extend(["host".into(), "serve".into()]);
    let args = crate::args::parse(host)?.context("Host arguments missing")?;
    ensure!(matches!(args.command, crate::args::Command::Serve), "Unexpected host command");
    // The desktop host opens folders and links for its UI, unlike a headless host.
    let remote = args.application.remote_options(std::env::current_exe()?, true)?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let serving = Serving {
        data: args.data_dir.clone(),
        helper: args.helper.clone(),
        digest: args.helper_sha256,
    };
    watch_ui(ui, relaunch, serving);
    LocalBootstrap::serve_foreground(
        &args.data_dir,
        &args.helper,
        args.helper_sha256,
        remote,
        &runtime,
        |unavailable| {
            if unavailable.is_some() {
                eprintln!("Local host ready. Remote unavailable: native approval storage review is pending.");
            }
            let mut stdout = std::io::stdout();
            let _ = writeln!(stdout, "{}", LocalBootstrap::DESKTOP_HOST_READY);
            let _ = stdout.flush();
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
        let discover = async {
            let client = LocalClient::connect(&self.data, &self.helper, self.digest).await?;
            let reply = client.request(&DesktopLaunchRequest::Discover {}).await;
            let _ = client.disconnect().await;
            anyhow::Ok(matches!(reply?, DesktopLaunchReply::Route(_)))
        };
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
            return false;
        };
        runtime
            .block_on(tokio::time::timeout(std::time::Duration::from_secs(10), discover))
            .is_ok_and(|routed| routed.unwrap_or(false))
    }
}

fn watch_ui(ui: PathBuf, relaunch: Vec<OsString>, serving: Serving) {
    let watch = move || {
        let mut stdin = std::io::stdin().lock();
        let mut byte = [0; 1];
        loop {
            match stdin.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) if byte[0] == LocalBootstrap::DESKTOP_HOST_QUIT => return,
                Ok(_) => {}
            }
        }
        if !serving.routes_launches() {
            return;
        }
        let mut command = Command::new(&ui);
        command
            .args(&relaunch)
            .arg("--owned-host")
            .stdin(Stdio::null())
            // A startup token belongs to the launch that already used it.
            .env_remove("DESKTOP_STARTUP_ID")
            .env_remove("XDG_ACTIVATION_TOKEN");
        match command.spawn() {
            // Reap it; a second crash is not relaunched.
            Ok(mut child) => drop(child.wait()),
            Err(error) => eprintln!("Shellfox UI relaunch failed: {error:#}"),
        }
    };
    if let Err(error) = std::thread::Builder::new().name("desktop-ui-watch".into()).spawn(watch) {
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
        let ui = if cfg!(windows) { r"C:\Shellfox\shellfox.exe" } else { "/opt/shellfox/shellfox" };
        let plan = plan(&strings(&[ui, "2", "--desktop-dir", "/d", "--data-dir", "/h"])).unwrap();
        assert_eq!(plan.ui, PathBuf::from(ui));
        assert_eq!(plan.relaunch, strings(&["--desktop-dir", "/d"]));
        assert_eq!(plan.host, strings(&["--data-dir", "/h"]));
        assert!(super::plan(&strings(&[ui, "3", "--desktop-dir", "/d"])).is_err());
        assert!(super::plan(&strings(&["relative", "0"])).is_err());
        assert!(super::plan(&strings(&[ui])).is_err());
    }
}
