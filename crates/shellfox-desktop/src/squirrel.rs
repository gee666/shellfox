//! Squirrel.Windows lifecycle. Update.exe runs these hooks with a 15 s limit
//! while it installs, updates or removes an app-<version> directory. Electron
//! 0.9.15 registered the same shortcuts, CLI shims, user PATH entry and
//! Explorer verbs, so each hook takes them over or removes them.
use anyhow::{Context, Result, bail};
use std::{
    ffi::OsStr,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

/// The shipping Squirrel package ID. Isolated test identities (ShellfoxTest…)
/// install beside it and must never touch the shared PATH or Explorer verbs.
const PACKAGE_ID: &str = "shellfox";
const EXE: &str = "Shellfox.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Install,
    Updated,
    Uninstall,
}

struct Install {
    root: PathBuf,
    version_dir: PathBuf,
    exe: PathBuf,
    /// Squirrel names the install directory after the package ID.
    id: String,
}
impl Install {
    fn current() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let version_dir = exe.parent()?.to_owned();
        let root = version_dir.parent()?.to_owned();
        let id = root.file_name()?.to_str()?.to_owned();
        let versioned = version_dir
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("app-"));
        (versioned && root.join("Update.exe").is_file()).then_some(Self {
            root,
            version_dir,
            exe,
            id,
        })
    }
    fn shared(&self) -> bool {
        self.id.eq_ignore_ascii_case(PACKAGE_ID)
    }
    /// Shortcut file name: Squirrel uses the exe's ProductName.
    fn title(&self) -> &str {
        if self.shared() { "Shellfox" } else { &self.id }
    }
}

/// Handles `--squirrel-*` events. Returns true when the process must exit.
pub(crate) fn handle(event: &OsStr) -> Result<bool> {
    let event = match event.to_str() {
        Some("--squirrel-install") => Event::Install,
        Some("--squirrel-updated") => Event::Updated,
        Some("--squirrel-uninstall") => Event::Uninstall,
        Some("--squirrel-obsolete") => return Ok(true),
        _ => return Ok(false),
    };
    if !shellfox_service::app_paths::is_packaged() {
        return Ok(true);
    }
    let Some(install) = Install::current() else {
        return Ok(true);
    };
    // Run every step so one failure cannot leave shortcuts or verbs behind.
    let remove = event == Event::Uninstall;
    let mut failures = Vec::new();
    let mut step = |name: &str, result: Result<()>| {
        if let Err(error) = result {
            failures.push(format!("{name}: {error:#}"));
        }
    };
    step(
        "CLI shims",
        crate::cli_launcher::install_windows_shims(
            &install.root.join("bin"),
            &install.root,
            &install.version_dir,
            &install.exe,
            remove,
        ),
    );
    if install.shared() {
        let bin = install.root.join("bin");
        step("user PATH", user_path(&bin, remove));
        step(
            "Explorer",
            explorer(&ExplorerKeys::USER, &install.exe, remove),
        );
    }
    step("shortcuts", shortcuts(&install, event));
    if !failures.is_empty() {
        bail!("Squirrel {event:?} hook failed: {}", failures.join("; "));
    }
    Ok(true)
}

/// Taskbar pins and Squirrel shortcuts carry this ID. Windows groups our
/// windows under a pin only when the running process reports the same one.
pub(crate) fn set_app_user_model_id() {
    let Some(install) = Install::current() else {
        return;
    };
    let id = windows::core::HSTRING::from(format!("com.squirrel.{}.Shellfox", install.id));
    // SAFETY: the HSTRING outlives the call; Windows copies the ID.
    if let Err(error) =
        unsafe { windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(&id) }
    {
        eprintln!("Could not set the Windows app ID: {error}");
    }
}

/// Install creates the default Desktop and Start Menu shortcuts. Update only
/// refreshes the ones the user kept: Squirrel points their working directory
/// at the versioned folder it is about to delete.
fn shortcuts(install: &Install, event: Event) -> Result<()> {
    let mut command = Command::new(install.root.join("Update.exe"));
    command.creation_flags(CREATE_NO_WINDOW);
    match event {
        Event::Install => command.args(["--createShortcut", EXE]),
        Event::Uninstall => command.args(["--removeShortcut", EXE]),
        Event::Updated => {
            let locations = kept_shortcuts(install.title())?;
            if locations.is_empty() {
                return Ok(());
            }
            command.args(["--createShortcut", EXE, "-l", &locations.join(",")])
        }
    };
    let status = command.status().context("Start Update.exe")?;
    anyhow::ensure!(status.success(), "Update.exe exited with {status}");
    Ok(())
}

fn kept_shortcuts(title: &str) -> Result<Vec<&'static str>> {
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs};
    let link = format!("{title}.lnk");
    let mut locations = Vec::new();
    if known_folder(&FOLDERID_Desktop)?.join(&link).is_file() {
        locations.push("Desktop");
    }
    // Start Menu links live in Programs\<authors>\, or Programs\ for older Squirrel.
    let programs = known_folder(&FOLDERID_Programs)?;
    let in_programs = programs.join(&link).is_file()
        || std::fs::read_dir(&programs)?
            .filter_map(Result::ok)
            .any(|entry| entry.path().join(&link).is_file());
    if in_programs {
        locations.push("StartMenu");
    }
    Ok(locations)
}

fn known_folder(id: &windows::core::GUID) -> Result<PathBuf> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath},
    };
    // SAFETY: the returned buffer is owned by us and freed after copying.
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
        let value = path.to_string();
        CoTaskMemFree(Some(path.0 as _));
        Ok(PathBuf::from(value?))
    }
}

fn user_path(bin: &Path, remove: bool) -> Result<()> {
    use winreg::{
        RegValue,
        enums::{REG_EXPAND_SZ, REG_SZ},
        types::{FromRegValue, ToRegValue},
    };
    let (env, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Environment")?;
    let (value, kind) = match env.get_raw_value("Path") {
        Ok(raw) => {
            anyhow::ensure!(
                matches!(raw.vtype, REG_EXPAND_SZ | REG_SZ),
                "Unsupported user PATH registry type"
            );
            (String::from_reg_value(&raw)?, raw.vtype)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (String::new(), REG_EXPAND_SZ)
        }
        Err(error) => return Err(error.into()),
    };
    let path = bin.to_str().context("Invalid Shellfox PATH encoding")?;
    let joined = edit_path(&value, path, remove);
    if joined == value {
        return Ok(());
    }
    let mut new: RegValue = joined.to_reg_value();
    new.vtype = kind;
    env.set_raw_value("Path", &new)?;
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, WPARAM},
            UI::WindowsAndMessaging::{SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE},
        },
        core::w,
    };
    // SAFETY: the literal is valid and remains alive for the synchronous broadcast.
    unsafe {
        SendMessageTimeoutW(
            HWND(0xffff as _),
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("Environment").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        )
    };
    Ok(())
}

/// Adds or removes one PATH entry, matching it the way Electron did.
fn edit_path(value: &str, entry: &str, remove: bool) -> String {
    let normalized = |p: &str| {
        p.trim()
            .trim_matches('"')
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    let key = normalized(entry);
    let mut parts: Vec<_> = value
        .split(';')
        .filter(|p| !p.is_empty() && normalized(p) != key)
        .collect();
    if !remove {
        parts.push(entry);
    }
    parts.join(";")
}

/// HKCU verbs shared with Electron's native-v1 Explorer integration.
struct ExplorerKeys {
    folder: &'static str,
    background: &'static str,
    legacy: [&'static str; 2],
}
impl ExplorerKeys {
    const USER: Self = Self {
        folder: r"Software\Classes\Directory\shell\Shellfox",
        background: r"Software\Classes\Directory\Background\shell\Shellfox",
        legacy: [
            r"Software\Classes\Directory\shell\PiManager",
            r"Software\Classes\Directory\Background\shell\PiManager",
        ],
    };
}
const EXPLORER_OWNER: &str = "Shellfox/native-v1";

fn explorer_command(exe: &Path, background: bool) -> Result<String> {
    let exe = exe
        .to_str()
        .context("Invalid Shellfox executable encoding")?;
    anyhow::ensure!(
        !exe.contains('"') && !exe.chars().any(char::is_control),
        "Unsafe Shellfox executable path"
    );
    Ok(format!(
        "\"{exe}\" --new-session --cwd \"{}\\.\"",
        if background { "%V" } else { "%1" }
    ))
}

/// Points owned verbs at this version, creates missing ones, and never
/// touches a verb another program owns.
fn explorer(keys: &ExplorerKeys, exe: &Path, remove: bool) -> Result<()> {
    let user = RegKey::predef(HKEY_CURRENT_USER);
    // None: no key. Some(owned): key exists, owned by the expected marker or not.
    let owner = |name: &str, value: &str, expected: &str| -> Option<bool> {
        let key = user.open_subkey(name).ok()?;
        Some(key.get_value::<String, _>(value).ok().as_deref() == Some(expected))
    };
    for name in keys.legacy {
        if owner(name, "PiManagerOwner", "PiManager/native-v1") == Some(true) {
            user.delete_subkey_all(name)?;
        }
    }
    for (name, background) in [(keys.folder, false), (keys.background, true)] {
        match owner(name, "ShellfoxOwner", EXPLORER_OWNER) {
            Some(false) => continue,
            None if remove => continue,
            Some(true) if remove => {
                user.delete_subkey_all(name)?;
                continue;
            }
            _ => {}
        }
        let (key, _) = user.create_subkey(name)?;
        key.set_value("ShellfoxOwner", &EXPLORER_OWNER)?;
        key.set_value("", &"Open in Shellfox")?;
        key.set_value("MUIVerb", &"Open in Shellfox")?;
        if background {
            let _ = key.delete_value("MultiSelectModel");
        } else {
            key.set_value("MultiSelectModel", &"Single")?;
        }
        key.set_value(
            "Icon",
            &exe.to_str()
                .context("Invalid Shellfox executable encoding")?,
        )?;
        let (command, _) = key.create_subkey("command")?;
        command.set_value("", &explorer_command(exe, background)?)?;
    }
    use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    // SAFETY: no item pointers are passed for an association change.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    Ok(())
}

#[cfg(test)]
mod tests;
