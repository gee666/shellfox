//! `host service install|uninstall`: same-user startup, as in 0.9.x
//! (scripts/headless-service.mjs). Unit names and templates are unchanged, so
//! startup installed by 0.9.x can be removed here and vice versa. Templates come
//! from the anchor's `service/` directory, so startup follows future updates.
use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command};

pub fn service_name(user_data: &Path) -> String {
    let digest = Sha256::digest(user_data.to_string_lossy().as_bytes());
    format!("shellfox-host-{}", &format!("{digest:x}")[..12])
}

pub fn run(install: bool, startup: bool, user_data: &Path, data: &Path) -> Result<String> {
    if startup && !cfg!(windows) {
        bail!("--startup is only supported for Windows boot tasks.");
    }
    let root = crate::headless_layout::current_root()
        .context("Startup management requires an extracted headless package.")?;
    let anchor = crate::headless_layout::anchor(&root)?;
    let text = user_data
        .to_str()
        .context("User data must be valid Unicode.")?;
    ensure!(
        !text.contains(['\r', '\n', '\0']),
        "User data must be an absolute path without line breaks."
    );
    let name = service_name(user_data);
    if install {
        std::fs::create_dir_all(user_data)?;
    } else {
        stop_host(user_data, data);
    }
    let template = |file: &str| {
        std::fs::read_to_string(anchor.join("service").join(file))
            .with_context(|| format!("Missing service template {file}"))
    };
    let notice = platform::apply(install, startup, &name, &anchor, text, template)?;
    Ok(format!(
        "{notice}{}: {name}",
        if install { "install" } else { "uninstall" }
    ))
}

/// Uninstall stops the host gracefully first; no PID kills.
fn stop_host(user_data: &Path, data: &Path) {
    let stop = async {
        if let Ok(client) = crate::node_run::connect(data).await {
            client
                .execute(shellfox_service::experiment::ExperimentRequest::Stop {})
                .await?;
        } else if let Some(legacy) = crate::legacy_host::LegacyHost::connect(user_data).await {
            legacy.stop().await?;
        }
        anyhow::Ok(())
    };
    if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        let _ = runtime.block_on(stop);
    }
}

fn invoke(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("Could not run {program}"))?;
    ensure!(status.success(), "{program} failed with status {status}");
    Ok(())
}
#[cfg(unix)]
fn home() -> Result<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .context("HOME is not set")
}

#[cfg(any(target_os = "linux", test))]
pub fn systemd_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
}
#[cfg(any(target_os = "linux", test))]
pub fn render_systemd(template: &str, root: &str, data: &str) -> String {
    template
        .split('\n')
        .map(|line| {
            let value = |token: &str| {
                let escaped = systemd_escape(token);
                if line.starts_with("ExecStart=") {
                    escaped.replace('$', "$$")
                } else {
                    escaped
                }
            };
            line.replace("@INSTALL_ROOT@", &value(root))
                .replace("@DATA_ROOT@", &value(data))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[cfg(any(target_os = "macos", test))]
pub fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
#[cfg(unix)]
fn write_private(file: &Path, contents: &str) -> Result<()> {
    let _ = std::fs::remove_file(file);
    crate::headless_layout::write_private(file, contents.as_bytes(), true)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    pub fn apply(
        install: bool,
        _startup: bool,
        name: &str,
        root: &Path,
        data: &str,
        template: impl Fn(&str) -> Result<String>,
    ) -> Result<&'static str> {
        let directory = home()?.join(".config/systemd/user");
        let file = directory.join(format!("{name}.service"));
        if install {
            std::fs::create_dir_all(&directory)?;
            let root = root
                .to_str()
                .context("Installation path must be valid Unicode.")?;
            write_private(
                &file,
                &render_systemd(&template("shellfox-user.service.in")?, root, data),
            )?;
            invoke("systemctl", &["--user", "daemon-reload"])?;
            invoke("systemctl", &["--user", "enable", "--now", name])?;
        } else {
            invoke("systemctl", &["--user", "disable", "--now", name])?;
            let _ = std::fs::remove_file(&file);
            invoke("systemctl", &["--user", "daemon-reload"])?;
        }
        Ok("")
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    pub fn apply(
        install: bool,
        _startup: bool,
        name: &str,
        root: &Path,
        data: &str,
        template: impl Fn(&str) -> Result<String>,
    ) -> Result<&'static str> {
        let label = format!("com.shellfox.{name}");
        let directory = home()?.join("Library/LaunchAgents");
        let file = directory.join(format!("{label}.plist"));
        let plist = file
            .to_str()
            .context("LaunchAgent path must be valid Unicode.")?;
        let uid = String::from_utf8(Command::new("/usr/bin/id").arg("-u").output()?.stdout)?;
        let domain = format!("gui/{}", uid.trim());
        if install {
            std::fs::create_dir_all(&directory)?;
            let root = root
                .to_str()
                .context("Installation path must be valid Unicode.")?;
            let text = template("com.shellfox.host.plist.in")?
                .replace("com.shellfox.host", &label)
                .replace("@INSTALL_ROOT@", &xml_escape(root))
                .replace("@DATA_ROOT@", &xml_escape(data));
            write_private(&file, &text)?;
            invoke("plutil", &["-lint", plist])?;
            invoke("launchctl", &["bootstrap", &domain, plist])?;
        } else {
            invoke("launchctl", &["bootout", &domain, plist])?;
            let _ = std::fs::remove_file(&file);
        }
        Ok("")
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    fn quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }
    pub fn apply(
        install: bool,
        startup: bool,
        name: &str,
        root: &Path,
        data: &str,
        _template: impl Fn(&str) -> Result<String>,
    ) -> Result<&'static str> {
        let task = quote(&format!("Shellfox-{name}"));
        let script = if install {
            let runner = root.join("service").join("run-host.ps1");
            let runner = runner
                .to_str()
                .context("Installation path must be valid Unicode.")?;
            let arguments = format!("-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{runner}\" -UserData \"{data}\"");
            let trigger = if startup {
                "$identity=[System.Security.Principal.WindowsIdentity]::GetCurrent(); if(-not ([System.Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Boot task installation requires an elevated shell of the intended user'}; $trigger=New-ScheduledTaskTrigger -AtStartup; $principal=New-ScheduledTaskPrincipal -UserId $user -LogonType S4U -RunLevel Limited; "
            } else {
                "$trigger=New-ScheduledTaskTrigger -AtLogOn -User $user; $principal=New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited; "
            };
            format!(
                "$ErrorActionPreference='Stop'; $user=[System.Security.Principal.WindowsIdentity]::GetCurrent().Name; \
                 $action=New-ScheduledTaskAction -Execute (Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe') -Argument {}; {trigger}\
                 $settings=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1); \
                 Register-ScheduledTask -TaskName {task} -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null; Start-ScheduledTask -TaskName {task}",
                quote(&arguments)
            )
        } else {
            format!("$ErrorActionPreference='Stop'; Unregister-ScheduledTask -TaskName {task} -Confirm:$false")
        };
        invoke(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        )?;
        Ok(match (install, startup) {
            (true, true) => "Windows unattended boot task, S4U limited token, not an SCM service.\n",
            (true, false) => "Windows login-autostart task, not an SCM service. Runs only while this user is logged in.\n",
            _ => "",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_escaping_match_the_node_installer() {
        // sha256("/srv/shellfox-data") prefix, as scripts/headless-service.mjs computes it.
        let expected = format!("{:x}", Sha256::digest(b"/srv/shellfox-data"));
        assert_eq!(
            service_name(Path::new("/srv/shellfox-data")),
            format!("shellfox-host-{}", &expected[..12])
        );
        let unit = render_systemd(
            "ExecStart=\"@INSTALL_ROOT@/x\" \"@DATA_ROOT@\"\nWorkingDirectory=@DATA_ROOT@",
            "/opt/a$b",
            "/d/100%",
        );
        assert_eq!(
            unit,
            "ExecStart=\"/opt/a$$b/x\" \"/d/100%%\"\nWorkingDirectory=/d/100%%"
        );
        assert_eq!(xml_escape("a&<'\""), "a&amp;&lt;&apos;&quot;");
    }
}
