//! Tells the user that terminals run without a window, once the host stops
//! relaunching a UI that keeps crashing. Best effort: a missing notifier only
//! leaves the line in the host's log.

pub(super) fn text(terminals: usize) -> String {
    let running = if terminals == 1 {
        "1 terminal is".to_owned()
    } else {
        format!("{terminals} terminals are")
    };
    format!("Shellfox's window keeps closing. {running} still running. Run shellfox to see them.")
}

pub(super) fn show(terminals: usize) {
    let text = text(terminals);
    eprintln!("{text}");
    if let Err(error) = os::show(&text) {
        eprintln!("Shellfox notification failed: {error:#}");
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod os {
    use anyhow::{ensure, Result};
    use std::process::{Command, Stdio};
    /// notify-send, else the same D-Bus call through gdbus.
    pub fn show(text: &str) -> Result<()> {
        let sent = run(Command::new("notify-send").args([
            "--app-name=Shellfox",
            "--icon=shellfox",
            "--urgency=critical",
            "Shellfox",
            text,
        ]));
        if sent.is_ok() {
            return sent;
        }
        run(Command::new("gdbus").args([
            "call",
            "--session",
            "--dest=org.freedesktop.Notifications",
            "--object-path=/org/freedesktop/Notifications",
            "--method=org.freedesktop.Notifications.Notify",
            "Shellfox",
            "0",
            "shellfox",
            "Shellfox",
            text,
            "[]",
            "{'urgency': <byte 2>}",
            "-1",
        ]))
    }
    fn run(command: &mut Command) -> Result<()> {
        let status = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        ensure!(status.success(), "notifier exited with {status}");
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod os {
    use anyhow::{ensure, Result};
    /// The text goes in as an argument, never into the script.
    pub fn show(text: &str) -> Result<()> {
        let status = std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 1 of argv) with title \"Shellfox\"",
                "-e",
                "end run",
                text,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        ensure!(status.success(), "osascript exited with {status}");
        Ok(())
    }
}

#[cfg(windows)]
mod os {
    use anyhow::Result;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    };
    /// A message box on its own thread: the host keeps serving meanwhile.
    pub fn show(text: &str) -> Result<()> {
        let wide = |value: &str| value.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (text, title) = (wide(text), wide("Shellfox"));
        std::thread::Builder::new()
            .name("ui-gone-notice".into())
            .spawn(move || {
                // SAFETY: both strings are NUL-terminated and outlive the call.
                unsafe {
                    MessageBoxW(
                        std::ptr::null_mut(),
                        text.as_ptr(),
                        title.as_ptr(),
                        MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST,
                    )
                };
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_text_counts_the_terminals() {
        assert_eq!(
            super::text(1),
            "Shellfox's window keeps closing. 1 terminal is still running. Run shellfox to see them."
        );
        assert!(super::text(3).contains(" 3 terminals are still running."));
    }
}
