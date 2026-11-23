use super::PowerError;
use crate::system::{NativeSystemFiles, SystemFile, SystemFiles};
use std::{
    ffi::OsString,
    io,
    os::windows::{ffi::OsStringExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

pub(super) const JOURNAL: &str = "keep-awake-lid.txt";
const KEEP_AWAKE: Values = Values { ac: 0, dc: 0 };

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Values {
    ac: u32,
    dc: u32,
}

#[derive(Debug, Eq, PartialEq)]
struct Journal {
    scheme: Option<Uuid>,
    original: Values,
}
impl Journal {
    fn parse(bytes: &[u8]) -> Result<Self, PowerError> {
        let invalid =
            || PowerError("Invalid keep-awake lid journal; no power settings were changed.".into());
        if bytes.len() > 80 {
            return Err(invalid());
        }
        let mut parts = std::str::from_utf8(bytes)
            .map_err(|_| invalid())?
            .trim_start_matches('\u{feff}')
            .trim()
            .split(',');
        let first = parts.next().ok_or_else(invalid)?;
        let (scheme, ac) = match Uuid::parse_str(first) {
            Ok(scheme) => (Some(scheme), parts.next().ok_or_else(invalid)?),
            Err(_) => (None, first),
        };
        let parse = |value: &str| {
            value
                .parse::<u32>()
                .ok()
                .filter(|value| *value <= 3)
                .ok_or_else(invalid)
        };
        let original = Values {
            ac: parse(ac)?,
            dc: parse(parts.next().ok_or_else(invalid)?)?,
        };
        if parts.next().is_some() {
            return Err(invalid());
        }
        Ok(Self { scheme, original })
    }
}

#[derive(Clone, Copy)]
enum CommandKind {
    Query,
    SetAc(u32),
    SetDc(u32),
    Apply,
}
fn arguments(scheme: Uuid, kind: CommandKind) -> Vec<String> {
    let scheme = scheme.to_string();
    let mut args = vec![
        match kind {
            CommandKind::Query => "/qh",
            CommandKind::SetAc(_) => "/setacvalueindex",
            CommandKind::SetDc(_) => "/setdcvalueindex",
            CommandKind::Apply => "/setactive",
        }
        .to_owned(),
        scheme,
    ];
    if !matches!(kind, CommandKind::Apply) {
        args.extend(["SUB_BUTTONS".into(), "LIDACTION".into()]);
    }
    if let CommandKind::SetAc(value) | CommandKind::SetDc(value) = kind {
        args.push(value.to_string());
    }
    args
}

fn parse_query(bytes: &[u8]) -> Result<Values, PowerError> {
    // Tokens are invariant even when Windows localizes the field names.
    let mut indices = bytes.split(u8::is_ascii_whitespace).filter_map(|part| {
        let hex = part.strip_prefix(b"0x")?;
        if hex.len() != 8 {
            return None;
        }
        u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()
    });
    match (indices.next(), indices.next(), indices.next()) {
        (Some(ac @ 0..=3), Some(dc @ 0..=3), None) => Ok(Values { ac, dc }),
        _ => Err(PowerError(
            "PowerCfg did not report both lid-close settings.".into(),
        )),
    }
}

struct PowerCfg(PathBuf);
impl PowerCfg {
    fn new() -> Result<Self, PowerError> {
        let mut buffer = [0; 260];
        let length =
            unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        if length == 0 || length >= buffer.len() {
            return Err(PowerError::operation(
                "GetSystemDirectoryW",
                io::Error::last_os_error(),
            ));
        }
        Ok(Self(
            PathBuf::from(OsString::from_wide(&buffer[..length])).join("powercfg.exe"),
        ))
    }
    fn run(
        &self,
        args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
    ) -> Result<Vec<u8>, PowerError> {
        let output = Command::new(&self.0)
            .args(args)
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|error| PowerError::operation("PowerCfg", error))?;
        if !output.status.success() {
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            let message: String = diagnostic.trim().chars().take(300).collect();
            return Err(PowerError::operation(
                "PowerCfg",
                if message.is_empty() {
                    output.status.to_string()
                } else {
                    message
                },
            ));
        }
        Ok(output.stdout)
    }
    fn active_scheme(&self) -> Result<Uuid, PowerError> {
        let output = self.run(["/getactivescheme"])?;
        String::from_utf8_lossy(&output)
            .split_whitespace()
            .find_map(|token| Uuid::parse_str(token).ok())
            .ok_or_else(|| PowerError("PowerCfg did not report an active power scheme.".into()))
    }
    fn read(&self, scheme: Uuid) -> Result<Values, PowerError> {
        parse_query(&self.run(arguments(scheme, CommandKind::Query))?)
    }
    fn set(&self, scheme: Uuid, values: Values) -> Result<(), PowerError> {
        self.run(arguments(scheme, CommandKind::SetAc(values.ac)))?;
        self.run(arguments(scheme, CommandKind::SetDc(values.dc)))?;
        if self.active_scheme()? == scheme {
            self.run(arguments(scheme, CommandKind::Apply))?;
        }
        if self.read(scheme)? != values {
            return Err(PowerError(
                "Could not verify Windows lid-close settings.".into(),
            ));
        }
        Ok(())
    }
}

fn read_journal(path: &Path) -> Result<Option<SystemFile>, PowerError> {
    NativeSystemFiles
        .read(path)
        .map_err(|error| PowerError(error.message))
}
fn replace(path: &Path, old: Option<&SystemFile>, bytes: Option<&[u8]>) -> Result<(), PowerError> {
    NativeSystemFiles
        .replace(path, old, bytes, false)
        .map_err(|error| PowerError(error.message))
}
fn restore_values(
    record: Journal,
    active: impl FnOnce() -> Result<Uuid, PowerError>,
    apply: impl FnOnce(Uuid, Values) -> Result<(), PowerError>,
    forget: impl FnOnce() -> Result<(), PowerError>,
) -> Result<(), PowerError> {
    let scheme = match record.scheme {
        Some(scheme) => scheme,
        None => active()?,
    };
    apply(scheme, record.original)?;
    forget()
}
fn restore(path: &Path, file: &SystemFile, power: &PowerCfg) -> Result<(), PowerError> {
    restore_values(
        Journal::parse(&file.bytes)?,
        || power.active_scheme(),
        |scheme, values| power.set(scheme, values),
        || replace(path, Some(file), None),
    )
}

pub(super) fn verify_existing_file(path: &Path) -> Result<(), PowerError> {
    read_journal(path)?.ok_or_else(|| PowerError("Electron ownership lock is missing.".into()))?;
    Ok(())
}

pub(super) fn recover(path: &Path) -> Result<(), PowerError> {
    if let Some(file) = read_journal(path)? {
        restore(path, &file, &PowerCfg::new()?)?;
    }
    Ok(())
}

pub(super) fn activate(path: &Path) -> Result<(), PowerError> {
    let power = PowerCfg::new()?;
    if let Some(file) = read_journal(path)? {
        restore(path, &file, &power)?;
    }
    let scheme = power.active_scheme()?;
    let original = power.read(scheme)?;
    let bytes = format!("{scheme},{},{}\n", original.ac, original.dc);
    replace(path, None, Some(bytes.as_bytes()))?;
    if let Err(error) = power.set(scheme, KEEP_AWAKE) {
        return match recover(path) {
            Ok(()) => Err(error),
            Err(restore_error) => Err(PowerError(format!(
                "{error}; lid restore needs retry: {restore_error}"
            ))),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests;
