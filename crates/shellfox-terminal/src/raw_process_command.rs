use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) const MAX_CONFIG_BYTES: usize = 1024 * 1024;

// Deliberately no Debug: environment, argv and cwd are not diagnostics.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawCommand {
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: String,
    #[serde(deserialize_with = "environment")]
    pub env: BTreeMap<String, String>,
}
impl RawCommand {
    pub fn validate(&self) -> Result<()> {
        text(&self.executable)?;
        text(&self.cwd)?;
        ensure!(
            !self.executable.is_empty(),
            "Missing automation executable."
        );
        ensure!(
            Path::new(&self.cwd).is_absolute() && Path::new(&self.cwd).is_dir(),
            "Automation cwd must be an existing absolute directory."
        );
        ensure!(self.args.len() <= 4096, "Too many automation arguments.");
        for arg in &self.args {
            text(arg)?;
        }
        ensure!(
            self.env.len() <= 1024,
            "Too many automation environment entries."
        );
        let mut units = 1;
        for (key, value) in &self.env {
            ensure!(
                !key.is_empty()
                    && key.encode_utf16().count() <= 1024
                    && !key.chars().any(|c| c < ' ' || c == '='),
                "Invalid automation environment key."
            );
            text(value)?;
            units += key.encode_utf16().count() + value.encode_utf16().count() + 2;
        }
        if cfg!(windows) {
            // Use the same native key comparison as actual Command.envs. Unicode
            // uppercase expansion is not Windows ordinal case-insensitive equality.
            let mut environment = std::process::Command::new("__shellfox_validation_only__");
            environment.env_clear().envs(&self.env);
            ensure!(
                environment.get_envs().count() == self.env.len(),
                "Duplicate automation environment key."
            );
            ensure!(
                units <= 32767,
                "Automation environment exceeds Windows limit."
            );
            ensure!(
                !self.cwd.chars().any(|c| c < ' ' || c == '"'),
                "Invalid automation cwd."
            );
            ensure!(
                !self.executable.chars().any(|c| c < ' ' || c == '"'),
                "Invalid automation executable."
            );
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_CONFIG_BYTES,
            "Automation launch exceeds its bound."
        );
        Ok(())
    }
    pub(crate) fn resolved(&self) -> Result<Self> {
        self.validate()?;
        let mut command = self.clone();
        command.executable = resolve(self)?
            .to_str()
            .context("Automation executable is not Unicode.")?
            .to_owned();
        #[cfg(windows)]
        if windows_powershell(Path::new(&command.executable)) {
            // Like pwsh does for its own children: a PowerShell 7 module path
            // from the caller hides Windows PowerShell's modules, PSReadLine
            // included. Unset, Windows PowerShell builds its own default.
            command
                .env
                .retain(|key, _| !key.eq_ignore_ascii_case("PSModulePath"));
        }
        #[cfg(windows)]
        ensure!(
            windows_command_units(&command) < 32767,
            "Automation command line exceeds Windows limit."
        );
        Ok(command)
    }
}
/// powershell.exe, Windows PowerShell 5.1.
#[cfg(windows)]
fn windows_powershell(executable: &Path) -> bool {
    executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("powershell.exe"))
}
fn text(value: &str) -> Result<()> {
    ensure!(
        value.encode_utf16().count() <= 32768 && !value.contains('\0'),
        "Invalid automation text."
    );
    Ok(())
}
fn environment<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Entries;
    impl<'de> serde::de::Visitor<'de> for Entries {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("unique environment entries")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            let mut names = std::collections::HashSet::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if result.len() >= 1024 || !names.insert(key.clone()) {
                    return Err(serde::de::Error::custom(
                        "Duplicate or oversized environment.",
                    ));
                }
                result.insert(key, value);
            }
            Ok(result)
        }
    }
    de.deserialize_map(Entries)
}
#[cfg(windows)]
fn resolve(command: &RawCommand) -> Result<PathBuf> {
    let value = |key: &str| {
        command
            .env
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    };
    let extensions: Vec<_> = value("PATHEXT")
        .unwrap_or(".COM;.EXE")
        .split(';')
        .filter(|s| s.eq_ignore_ascii_case(".exe") || s.eq_ignore_ascii_case(".com"))
        .collect();
    let mut suffixes = vec![""];
    if Path::new(&command.executable).extension().is_none() {
        suffixes.extend(extensions);
    }
    let explicit =
        Path::new(&command.executable).is_absolute() || command.executable.contains(['/', '\\']);
    let mut dirs = vec![PathBuf::from(&command.cwd)];
    if !explicit {
        dirs.extend(value("PATH").unwrap_or("").split(';').filter_map(|s| {
            let directory = s
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(s);
            (!directory.is_empty()).then(|| Path::new(&command.cwd).join(directory))
        }));
    }
    for dir in dirs {
        for suffix in &suffixes {
            let path = dir.join(format!("{}{suffix}", command.executable));
            let native = path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("exe") || s.eq_ignore_ascii_case("com"));
            if native && path.is_file() {
                return Ok(path);
            }
        }
    }
    anyhow::bail!(
        "Automation executable not found. Invoke batch scripts explicitly through cmd.exe."
    )
}
#[cfg(target_os = "linux")]
fn resolve(command: &RawCommand) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let program = Path::new(&command.executable);
    let candidates = if program.is_absolute() || command.executable.contains('/') {
        vec![Path::new(&command.cwd).join(program)]
    } else {
        command
            .env
            .get("PATH")
            .map(|path| {
                std::env::split_paths(path)
                    .map(|path| Path::new(&command.cwd).join(path).join(program))
                    .collect()
            })
            .unwrap_or_else(|| {
                vec![
                    Path::new("/usr/bin").join(program),
                    Path::new("/bin").join(program),
                ]
            })
    };
    for candidate in candidates {
        if let Ok(metadata) = std::fs::metadata(&candidate) {
            if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                return std::fs::canonicalize(candidate).context("Resolve raw executable.");
            }
        }
    }
    anyhow::bail!("Automation executable not found in the explicit cwd/PATH.")
}
#[cfg(not(any(windows, target_os = "linux")))]
fn resolve(_command: &RawCommand) -> Result<PathBuf> {
    // Do not add std::Child/killpg here. Unix raw launch needs the pidfd/subreaper
    // or authenticated native supervisor, which this platform doesn't have yet.
    anyhow::bail!("Automation processes are not supported on this platform yet.")
}
#[cfg(windows)]
fn windows_command_units(command: &RawCommand) -> usize {
    // Rust Command uses CRT escaping, with quoting only for empty/space/tab
    // arguments. Count the encoded command including inserted escaping.
    fn argument(value: &str, force: bool) -> usize {
        let quote = force || value.is_empty() || value.contains([' ', '\t']);
        let mut units = usize::from(quote) * 2;
        let mut slashes = 0;
        for c in value.chars() {
            if c == '\\' {
                slashes += 1;
                continue;
            }
            units += slashes * if c == '"' { 2 } else { 1 };
            slashes = 0;
            units += c.len_utf16() + usize::from(c == '"');
        }
        units + slashes * if quote { 2 } else { 1 }
    }
    argument(&command.executable, true)
        + command
            .args
            .iter()
            .map(|s| 1 + argument(s, false))
            .sum::<usize>()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn windows_powershell_computes_its_own_module_path() {
        let system = crate::windows::system_directory().unwrap();
        let command = |executable: PathBuf| RawCommand {
            executable: executable.to_str().unwrap().into(),
            args: vec![],
            cwd: system.to_str().unwrap().into(),
            env: BTreeMap::from([
                (
                    "psmodulepath".into(),
                    r"C:\Program Files\PowerShell\7\Modules".into(),
                ),
                ("KEEP".into(), "1".into()),
            ]),
        };
        let powershell = command(system.join("WindowsPowerShell/v1.0/powershell.exe"))
            .resolved()
            .unwrap();
        assert_eq!(powershell.env.keys().collect::<Vec<_>>(), ["KEEP"]);
        let cmd = command(system.join("cmd.exe")).resolved().unwrap();
        assert_eq!(cmd.env.len(), 2);
    }
}
