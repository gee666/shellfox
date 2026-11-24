//! A host run from an extracted headless package puts that package's
//! `shellfox` launcher directory on PATH in its tabs, as 0.9 did.
#![cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{bail, ensure, Context, Result};
use fixture::Fixture;
use shellfox_service::{
    headless_layout::{arch, exe, normalize, platform, root_name},
    local::LocalClient,
    local_api::{LocalApiRequest, LocalApiResponse},
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[test]
fn headless_package_tabs_find_the_package_launcher_on_path() -> Result<()> {
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&tmp)?;
    let package = tempfile::Builder::new()
        .prefix("headless-path-")
        .tempdir_in(&tmp)?;
    let root = normalize(package.path()).join(root_name());
    let cli = extract(&root)?;
    let (helper, digest) = dev_helper::helper()?;
    let host = Fixture::start_as(cli, helper.clone(), digest)?;
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(tab_path(&host, &helper, digest));
    let stopped = host.stop();
    let path = result?;
    stopped?;
    let separator = if cfg!(windows) { ';' } else { ':' };
    let same = |entry: &str| {
        let entry = Path::new(entry.trim_end_matches(['\\', '/']));
        if cfg!(windows) {
            entry.as_os_str().eq_ignore_ascii_case(root.as_os_str())
        } else {
            entry == root
        }
    };
    ensure!(
        path.split(separator).any(same),
        "{} is not on the tab's PATH: {path}",
        root.display()
    );
    Ok(())
}

/// The package layout the release archive has: `bin/shellfox-cli`, `build.json`
/// and the `shellfox` launcher.
fn extract(root: &Path) -> Result<PathBuf> {
    let built = Path::new(env!("CARGO_BIN_EXE_shellfox-cli"));
    std::fs::create_dir_all(root.join("bin"))?;
    let cli = root.join("bin").join(exe("shellfox-cli"));
    std::fs::copy(built, &cli)?;
    // Windows debug builds load the terminal library from beside the executable.
    for entry in std::fs::read_dir(built.parent().context("build directory")?)? {
        let entry = entry?.path();
        if entry.extension().is_some_and(|e| e == "dll") {
            std::fs::copy(
                &entry,
                root.join("bin").join(entry.file_name().context("dll")?),
            )?;
        }
    }
    let version = env!("CARGO_PKG_VERSION");
    std::fs::write(
        root.join("build.json"),
        format!(
            r#"{{"product":"headless","version":"{version}","platform":"{}","arch":"{}"}}"#,
            platform(),
            arch()
        ),
    )?;
    if cfg!(windows) {
        std::fs::write(
            root.join("shellfox.cmd"),
            "@echo off\r\n\"%~dp0bin\\shellfox-cli.exe\" %*\r\nexit /b %errorlevel%\r\n",
        )?;
    } else {
        let launcher = root.join("shellfox");
        std::fs::write(
            &launcher,
            "#!/bin/sh\nexec \"$(dirname -- \"$0\")/bin/shellfox-cli\" \"$@\"\n",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(cli)
}

fn quote(word: &str) -> String {
    if cfg!(windows) {
        format!("'{}'", word.replace('\'', "''"))
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// PATH as a shell in a new tab of `host` sees it.
async fn tab_path(host: &Fixture, helper: &Path, digest: [u8; 32]) -> Result<String> {
    let session = host.json(&["session", "create", "--title", "Path", "--json"])?;
    let id = session["id"].as_str().context("id")?.to_owned();
    let profile = if cfg!(windows) {
        "windows-powershell"
    } else if cfg!(target_os = "macos") {
        "zsh"
    } else {
        "bash"
    };
    let created = host.json(&["terminals", "create", &id, "--profile", profile, "--json"])?;
    let tab = created["tabs"]
        .as_array()
        .and_then(|t| t.last())
        .context("tab")?["id"]
        .as_str()
        .context("tab id")?
        .parse()?;
    let out = host.scratch.path().join("path.txt");
    let client = LocalClient::connect(&host.data, helper, digest).await?;
    let attachment = match client
        .execute(LocalApiRequest::Attach {
            tab,
            takeover: false,
        })
        .await?
    {
        LocalApiResponse::Attachment(attachment) => attachment,
        _ => bail!("Unexpected attach response."),
    };
    let target = quote(&out.display().to_string());
    let command = if cfg!(windows) {
        format!("$env:PATH | Set-Content -Encoding utf8 -LiteralPath {target}\r")
    } else {
        format!("printf '%s\\n' \"$PATH\" > {target}\r")
    };
    client
        .execute(LocalApiRequest::Input {
            tab,
            attachment: attachment.clone(),
            bytes: command.into_bytes(),
        })
        .await?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let path = loop {
        if let Ok(text) = std::fs::read_to_string(&out) {
            if text.ends_with('\n') {
                break text.trim_start_matches('\u{feff}').trim().to_owned();
            }
        }
        ensure!(Instant::now() < deadline, "The tab never wrote its PATH.");
        // Keep the terminal's output drained while the shell runs.
        let _ = client
            .execute(LocalApiRequest::Poll {
                tab,
                attachment: attachment.clone(),
            })
            .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let _ = client.disconnect().await;
    Ok(path)
}
