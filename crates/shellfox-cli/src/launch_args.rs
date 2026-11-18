use anyhow::{bail, Context, Result};
use shellfox_service::desktop_launch::{DesktopLaunchIntent, LaunchAction, LaunchDisposition};
use std::path::{Path, PathBuf};

pub fn parse(words: &[&str]) -> Result<Option<DesktopLaunchIntent>> {
    let (action, options) = match words {
        [] => (None, &[][..]),
        ["--new-window"] => (None, words),
        ["start" | "add" | "new" | "create", rest @ ..] => (Some(()), rest),
        _ => return Ok(None),
    };
    let mut path = None;
    let mut new_window = false;
    for word in options {
        if *word == "--new-window" && !new_window {
            new_window = true;
        } else if !word.starts_with('-') && path.is_none() && action.is_some() {
            path = Some(*word);
        } else {
            bail!("Invalid start arguments. Use start [PATH] [--new-window].");
        }
    }
    let action = if action.is_some() {
        LaunchAction::NewSession {
            cwd: directory(path.unwrap_or("."), &std::env::current_dir()?)?,
        }
    } else {
        LaunchAction::Show {}
    };
    Ok(Some(DesktopLaunchIntent::new(
        action,
        if new_window {
            LaunchDisposition::NewWindow
        } else {
            LaunchDisposition::ExistingWindow
        },
    )))
}

pub fn directory(input: &str, invoking: &Path) -> Result<String> {
    if input.is_empty() || input.contains(['\0', '\r', '\n']) {
        bail!("Invalid directory.");
    }
    let input = Path::new(input);
    let directory: PathBuf = if input.is_absolute() {
        input.into()
    } else {
        invoking.join(input)
    };
    if !directory.is_absolute() || !directory.is_dir() {
        bail!("An existing local directory is required.");
    }
    // Match the existing CLI's Unix realpath behavior. Windows keeps the normal
    // drive/WSL spelling instead of introducing a verbatim device namespace.
    #[cfg(not(windows))]
    let directory = directory
        .canonicalize()
        .context("Cannot resolve directory.")?;
    let directory = directory
        .to_str()
        .context("Directory is not representable by the local protocol.")?
        .to_owned();
    shellfox_core::validation::cwd(&directory)?;
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bare_launch_only_shows_and_aliases_create() {
        assert!(matches!(
            parse(&[]).unwrap().unwrap().action,
            LaunchAction::Show {}
        ));
        for alias in ["start", "add", "new", "create"] {
            let intent = parse(&[alias]).unwrap().unwrap();
            assert!(matches!(intent.action, LaunchAction::NewSession { .. }));
            assert_eq!(intent.disposition, LaunchDisposition::ExistingWindow);
        }
        assert_eq!(
            parse(&["start", ".", "--new-window"])
                .unwrap()
                .unwrap()
                .disposition,
            LaunchDisposition::NewWindow
        );
    }
    #[test]
    fn caller_relative_path_and_invalid_arguments() {
        let invoking = std::env::current_dir().unwrap();
        let directory = directory(".", &invoking).unwrap();
        assert!(Path::new(&directory).is_absolute());
        assert!(Path::new(&directory).is_dir());
        for words in [
            &["start", "a", "b"][..],
            &["start", "--unknown"],
            &["start", "--new-window", "--new-window"],
        ] {
            assert!(parse(words).is_err());
        }
        assert!(parse(&["session", "list"]).unwrap().is_none());
    }
}
