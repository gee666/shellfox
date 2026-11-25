//! Native dialogs for startup, before any Shellfox window exists. Windows uses
//! a message box; Linux the first of zenity, kdialog and xmessage it finds;
//! macOS osascript. Texts go in as arguments, never into a script.

/// Shows an error. Best effort.
pub(crate) fn alert(message: &str) {
    os::alert(message);
}

/// Asks to `accept` or `cancel`. None if no dialog could be shown.
pub(crate) fn confirm(message: &str, accept: &str, cancel: &str) -> Option<bool> {
    os::confirm(message, accept, cancel)
}

#[cfg(windows)]
mod os {
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_YESNO,
            MESSAGEBOX_STYLE, MessageBoxW,
        },
        core::HSTRING,
    };
    fn show(
        message: &str,
        style: MESSAGEBOX_STYLE,
    ) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
        // Unparented native boxes match Electron's showErrorBox. This runs
        // outside any GPUI Window/App borrow and touches no other window.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(message),
                &HSTRING::from("Shellfox"),
                style | MB_SETFOREGROUND,
            )
        }
    }
    pub fn alert(message: &str) {
        show(message, MB_OK | MB_ICONERROR);
    }
    /// A message box has Yes and No only, so the accept label becomes the question.
    pub fn confirm(message: &str, accept: &str, _cancel: &str) -> Option<bool> {
        Some(
            show(
                &format!("{message}\n\n{accept}?"),
                MB_YESNO | MB_ICONWARNING,
            ) == IDYES,
        )
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod os {
    use std::process::{Command, Stdio};
    /// Exit 0 or 1 from the first tool that runs; None when none does.
    fn first(commands: Vec<Command>) -> Option<bool> {
        commands.into_iter().find_map(|mut command| {
            let status = command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .ok()?;
            match status.code() {
                Some(0) => Some(true),
                Some(1) => Some(false),
                _ => None,
            }
        })
    }
    fn command<const N: usize>(program: &str, args: [&str; N]) -> Command {
        let mut command = Command::new(program);
        command.args(args);
        command
    }
    pub fn alert(message: &str) {
        first(vec![
            command(
                "zenity",
                [
                    "--error",
                    "--title=Shellfox",
                    "--no-markup",
                    "--text",
                    message,
                ],
            ),
            command("kdialog", ["--title", "Shellfox", "--error", message]),
            command("xmessage", ["-center", "-buttons", "OK:0", message]),
        ]);
    }
    pub fn confirm(message: &str, accept: &str, cancel: &str) -> Option<bool> {
        let accept_label = format!("--ok-label={accept}");
        let cancel_label = format!("--cancel-label={cancel}");
        let buttons = format!(
            "{}:0,{}:1",
            accept.replace([',', ':'], " "),
            cancel.replace([',', ':'], " ")
        );
        first(vec![
            command(
                "zenity",
                [
                    "--question",
                    "--title=Shellfox",
                    "--no-markup",
                    "--icon-name=dialog-warning",
                    &accept_label,
                    &cancel_label,
                    "--text",
                    message,
                ],
            ),
            command(
                "kdialog",
                [
                    "--title",
                    "Shellfox",
                    "--yes-label",
                    accept,
                    "--no-label",
                    cancel,
                    "--warningyesno",
                    message,
                ],
            ),
            command("xmessage", ["-center", "-buttons", &buttons, message]),
        ])
    }
}

#[cfg(target_os = "macos")]
mod os {
    use std::process::{Command, Stdio};
    fn osascript(lines: &[&str], args: &[&str]) -> Option<std::process::Output> {
        let mut command = Command::new("/usr/bin/osascript");
        for line in lines {
            command.args(["-e", line]);
        }
        command
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
    }
    pub fn alert(message: &str) {
        osascript(
            &[
                "on run argv",
                "display alert \"Shellfox\" message (item 1 of argv) as critical",
                "end run",
            ],
            &[message],
        );
    }
    pub fn confirm(message: &str, accept: &str, cancel: &str) -> Option<bool> {
        let output = osascript(
            &[
                "on run argv",
                "set answer to display alert \"Shellfox\" message (item 1 of argv) as warning buttons {item 3 of argv, item 2 of argv} default button 2 cancel button 1",
                "return button returned of answer",
                "end run",
            ],
            &[message, accept, cancel],
        )?;
        // Cancel ends the script with an error; anything else unexpected too.
        let answer = String::from_utf8_lossy(&output.stdout);
        Some(output.status.success() && answer.trim() == accept)
    }
}
