//! Where a registered script lives and how it runs, from 0.9.15's
//! shellfox-cli-scripts.ts. Arguments stay literal: no shell ever parses them.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub path: String,
    /// A WSL guest path in this distribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distro: Option<String>,
}
fn posix_absolute(path: &str) -> bool {
    path.starts_with('/')
}
fn windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || path.starts_with("\\\\")
        || path.starts_with("//")
}
fn plain(text: &str) -> bool {
    !text.contains(['\0', '\r', '\n'])
}
impl Location {
    pub fn valid(&self) -> bool {
        plain(&self.path)
            && (posix_absolute(&self.path) || windows_absolute(&self.path))
            && self
                .distro
                .as_ref()
                .is_none_or(|d| !d.is_empty() && plain(d) && posix_absolute(&self.path))
    }
}

/// Set by the posix `shellfox` shim when it runs from WSL or Git Bash.
fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}
fn wsl() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(root).join("System32").join("wsl.exe")
}
/// stdout of a short helper such as wslpath or cygpath, bounded like 0.9's.
fn capture(command: &mut Command) -> Result<String> {
    let output = command
        .stdin(std::process::Stdio::null())
        .output()
        .context("Path conversion failed.")?;
    if output.stdout.len() + output.stderr.len() > 65536 {
        bail!("Path conversion output exceeded its limit.");
    }
    if !output.status.success() {
        let error: String = String::from_utf8_lossy(&output.stderr)
            .trim()
            .chars()
            .take(1000)
            .collect();
        bail!(
            "{}",
            if error.is_empty() {
                "Path conversion failed.".into()
            } else {
                error
            }
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// MSYS treats apostrophes as quoting, so every Git Bash argument is double-quoted.
#[cfg(windows)]
fn quote_msys(value: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            c => {
                out.push_str(&"\\".repeat(slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

pub fn resolve(input: &str) -> Result<Location> {
    if !plain(input) {
        bail!("Script paths cannot contain NUL or line breaks.");
    }
    if input.is_empty() {
        bail!("Missing script path.");
    }
    if cfg!(windows) {
        if let Some(distro) = env("SHELLFOX_CLI_DISTRO") {
            let cwd = env("SHELLFOX_CLI_WSL_CWD")
                .filter(|cwd| posix_absolute(cwd))
                .context("Missing WSL caller directory. Run shellfox from its WSL shim.")?;
            let guest = if windows_absolute(input) && !posix_absolute(input) {
                capture(Command::new(wsl()).args([
                    "--distribution",
                    &distro,
                    "--exec",
                    "wslpath",
                    "-u",
                    input,
                ]))?
            } else {
                posix_resolve(&cwd, input)
            };
            capture(Command::new(wsl()).args([
                "--distribution",
                &distro,
                "--exec",
                "/usr/bin/test",
                "-f",
                &guest,
            ]))?;
            return Ok(Location {
                path: guest,
                distro: Some(distro),
            });
        }
    }
    let file = git_bash_path(input)?.map_or_else(|| absolute(input), Ok)?;
    if !Path::new(&file).is_file() {
        bail!("Script path must refer to a file.");
    }
    if cfg!(windows) && matches!(extension(&file).as_str(), ".bat" | ".cmd") {
        bail!("Batch scripts are not supported. Use .ps1 or .sh to preserve literal arguments.");
    }
    Ok(Location {
        path: file,
        distro: None,
    })
}

/// `path.posix.resolve(cwd, input)`.
fn posix_resolve(cwd: &str, input: &str) -> String {
    let joined = if posix_absolute(input) {
        input.to_owned()
    } else {
        format!("{cwd}/{input}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}
fn absolute(input: &str) -> Result<String> {
    let path = crate::node_args::resolve(&std::path::absolute(input)?);
    path.into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("Script path must be valid Unicode."))
}
#[cfg(windows)]
fn git_bash_path(input: &str) -> Result<Option<String>> {
    if env("SHELLFOX_CLI_GIT_BASH").is_none() || windows_absolute(input) {
        return Ok(None);
    }
    let bash = git_bash()?;
    let cygpath = bash.with_file_name("cygpath.exe");
    let mut command = Command::new(cygpath);
    use std::os::windows::process::CommandExt;
    command
        .raw_arg(quote_msys("-wa"))
        .raw_arg(quote_msys("--"))
        .raw_arg(quote_msys(input));
    if let Some(cwd) = env("SHELLFOX_CLI_GIT_CWD") {
        command.current_dir(cwd);
    }
    capture(&mut command).map(Some)
}
#[cfg(not(windows))]
fn git_bash_path(_: &str) -> Result<Option<String>> {
    Ok(None)
}
fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_default()
}
#[cfg(windows)]
fn git_bash() -> Result<PathBuf> {
    if let Some(bash) = env("SHELLFOX_CLI_GIT_BASH") {
        if !plain(&bash) || !windows_absolute(&bash) {
            bail!("Git Bash interpreter must be an absolute path.");
        }
        let bash = PathBuf::from(bash);
        if bash.is_file() {
            return Ok(bash);
        }
    }
    let mut candidates: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("Git").join("bin").join("bash.exe"))
        .collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local)
                .join("Programs")
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
    }
    candidates.into_iter().find(|c| c.is_file()).context(
        "Running .sh scripts on Windows requires Git Bash. Install Git, run from Git Bash, or register the script inside WSL.",
    )
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() {
                char::from(TABLE[(n >> shift) as usize & 63])
            } else {
                '='
            });
        }
    }
    out
}

/// PowerShell receives the path and arguments as data, never as script text.
fn powershell_command(path: &str, args: &[String]) -> String {
    let payload = base64(
        serde_json::json!({ "path": path, "args": args })
            .to_string()
            .as_bytes(),
    );
    let code = format!("$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; $global:LASTEXITCODE=0; try {{ $p=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{payload}')) | ConvertFrom-Json; $scriptArgs=@($p.args); & $p.path @scriptArgs; $scriptOk=$?; if ($LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}; if (-not $scriptOk) {{ exit 1 }}; exit 0 }} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}");
    let utf16: Vec<u8> = code.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&utf16)
}

fn invocation(location: &Location, args: &[String]) -> Result<Command> {
    if args.iter().any(|a| a.contains('\0')) {
        bail!("Arguments cannot contain NUL.");
    }
    if !plain(&location.path) {
        bail!("Script paths cannot contain NUL or line breaks.");
    }
    let cwd: PathBuf = match env("SHELLFOX_CLI_GIT_CWD").filter(|_| cfg!(windows)) {
        Some(cwd) => cwd.into(),
        None => std::env::current_dir()?,
    };
    let extension = extension(&location.path);
    let mut command;
    if let Some(distro) = &location.distro {
        if !cfg!(windows) {
            bail!("This command belongs to WSL. Run it with Shellfox on Windows.");
        }
        let guest_cwd = match (env("SHELLFOX_CLI_DISTRO"), env("SHELLFOX_CLI_WSL_CWD")) {
            (Some(current), Some(cwd)) if &current == distro => cwd,
            _ => capture(
                Command::new(wsl())
                    .args(["--distribution", distro, "--exec", "wslpath", "-u"])
                    .arg(&cwd),
            )?,
        };
        command = Command::new(wsl());
        command.args(["--distribution", distro, "--cd", &guest_cwd, "--exec"]);
        if extension == ".sh" {
            command.args(["/bin/bash", "--"]);
        }
        command.arg(&location.path).args(args);
    } else if extension == ".sh" {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Forward slashes work in Git Bash and keep spaces without bash -c.
            command = Command::new(git_bash()?);
            command
                .env("MSYS_ARG_CONV_EXCL", "*")
                .raw_arg(quote_msys("--"))
                .raw_arg(quote_msys(&location.path.replace('\\', "/")));
            for arg in args {
                command.raw_arg(quote_msys(arg));
            }
        }
        #[cfg(not(windows))]
        {
            command = Command::new("bash");
            command.arg("--").arg(&location.path).args(args);
        }
    } else if extension == ".ps1" {
        let program: OsString = if cfg!(windows) {
            let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            PathBuf::from(root)
                .join("System32/WindowsPowerShell/v1.0/powershell.exe")
                .into()
        } else {
            "pwsh".into()
        };
        command = Command::new(program);
        command.args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
        ]);
        command.arg(powershell_command(&location.path, args));
    } else {
        if cfg!(windows) && extension != ".exe" && extension != ".com" {
            bail!("Windows custom scripts must use .sh or .ps1, or be .exe/.com programs. Batch scripts are not supported.");
        }
        command = Command::new(&location.path);
        command.args(args);
    }
    command.current_dir(cwd).env_remove("ELECTRON_RUN_AS_NODE");
    Ok(command)
}

/// Runs in the foreground with inherited stdio and returns its exit code.
/// Ctrl+C reaches the script; Shellfox waits for it rather than exiting first.
pub fn execute(location: &Location, args: &[String]) -> Result<i32> {
    let mut child = invocation(location, args)?.spawn()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let status = runtime.block_on(async {
        let wait = tokio::task::spawn_blocking(move || child.wait());
        tokio::pin!(wait);
        loop {
            tokio::select! {
                status = &mut wait => break status,
                _ = tokio::signal::ctrl_c() => {}
            }
        }
    })??;
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return Ok(match signal {
            2 => 130,
            15 => 143,
            _ => 1,
        });
    }
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn powershell_payload_round_trips_literal_arguments() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert!(!powershell_command("C:\\a b\\x.ps1", &["'; rm -rf /".into()]).contains("rm"));
        assert_eq!(posix_resolve("/home/u", "../x/./y.sh"), "/home/x/y.sh");
        assert!(Location {
            path: "/a".into(),
            distro: Some("Ubuntu".into())
        }
        .valid());
        assert!(!Location {
            path: "C:\\a".into(),
            distro: Some("Ubuntu".into())
        }
        .valid());
        assert!(!Location {
            path: "relative".into(),
            distro: None
        }
        .valid());
    }
    #[cfg(windows)]
    #[test]
    fn msys_quoting_survives_quotes_and_trailing_backslashes() {
        assert_eq!(quote_msys("it's"), "\"it's\"");
        assert_eq!(quote_msys("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_msys("dir\\"), "\"dir\\\\\"");
    }
}
