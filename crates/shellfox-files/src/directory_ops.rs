use crate::{directory_wsl::SharePath, paths, ErrorCode, FileError, FileService, Location, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const DIRECTORY_COMPLETION_LIMIT: usize = 100;
pub const DIRECTORY_COMPLETION_BYTES: usize = 65536;
/// Host-authoritative context, not a deserialized frontend cwd hint.
/// A launch cwd must not be labeled as the live cwd of a running terminal.
#[derive(Debug, Clone)]
pub struct FileContext {
    pub cwd: String,
    pub home: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryResolved {
    pub cwd: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryCompletions {
    pub matches: Vec<String>,
}

fn unquote(value: &str) -> Result<String> {
    if value.len() > 32767 || value.chars().any(char::is_control) {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Invalid directory prompt path.",
        ));
    }
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && (trimmed.starts_with('"') && trimmed.ends_with('"')
            || trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        Ok(trimmed[1..trimmed.len() - 1].to_owned())
    } else {
        Ok(value.to_owned())
    }
}
/// The lexically first matching directory names, without holding a whole huge directory.
pub(crate) struct Candidates {
    needle: String,
    names: Vec<String>,
}
impl Candidates {
    fn new(fragment: &str) -> Self {
        Self {
            needle: comparable(fragment),
            names: Vec::new(),
        }
    }
    /// Checked before the entry's metadata is read.
    pub(crate) fn wants(&self, name: &str) -> bool {
        comparable(name).starts_with(&self.needle) && paths::validate(name).is_ok()
    }
    pub(crate) fn keep(&mut self, name: String) {
        let position = self
            .names
            .binary_search_by(|other| other.encode_utf16().cmp(name.encode_utf16()))
            .unwrap_or_else(|position| position);
        if position < DIRECTORY_COMPLETION_LIMIT {
            self.names.insert(position, name);
            self.names.truncate(DIRECTORY_COMPLETION_LIMIT);
        }
    }
}
fn comparable(name: &str) -> String {
    if cfg!(windows) {
        name.to_lowercase()
    } else {
        name.to_owned()
    }
}
fn normalized(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}

impl FileService {
    pub fn resolve_location(&self, input: &Location, context: &FileContext) -> Result<Location> {
        self.require_path_resolution()?;
        paths::validate(&input.path)?;
        let path = if input.path == "~"
            || input.path.starts_with("~/")
            || cfg!(windows) && input.path.starts_with("~\\")
        {
            let home = context.home.as_deref().ok_or_else(|| {
                FileError::new(
                    ErrorCode::Unsupported,
                    "The host home directory is unavailable.",
                )
            })?;
            paths::validate(home)?;
            if !Path::new(home).is_absolute() {
                return Err(FileError::denied());
            }
            Path::new(home).join(input.path.get(2..).unwrap_or(""))
        } else if Path::new(&input.path).is_absolute() {
            PathBuf::from(&input.path)
        } else {
            paths::validate(&context.cwd)?;
            if !Path::new(&context.cwd).is_absolute() {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    "Authoritative working directory evidence is unavailable.",
                ));
            }
            Path::new(&context.cwd).join(&input.path)
        };
        let absolute = paths::display(&normalized(&path))?;
        // Resolve only within this existing capability. Context can never grant another root.
        self.base(&absolute)?;
        Ok(Location { path: absolute })
    }
    fn prompt_location(&self, value: &str, context: Option<&FileContext>) -> Result<Location> {
        self.require_path_resolution()?;
        let value = if value.is_empty() { "." } else { value };
        let location = Location {
            path: value.to_owned(),
        };
        if let Some(context) = context {
            self.resolve_location(&location, context)
        } else if value == "~"
            || value.starts_with("~/")
            || cfg!(windows) && value.starts_with("~\\")
        {
            Err(FileError::new(
                ErrorCode::Unsupported,
                "Supply the host-authoritative home context.",
            ))
        } else {
            self.base(value)?;
            Ok(location)
        }
    }
    pub fn resolve_directory(&self, input: &Location) -> Result<DirectoryResolved> {
        self.resolve_directory_with_context(input, None)
    }
    pub fn resolve_directory_with_context(
        &self,
        input: &Location,
        context: Option<&FileContext>,
    ) -> Result<DirectoryResolved> {
        self.require_path_resolution()?;
        let value = unquote(&input.path)?;
        if let Some(share) = SharePath::parse(&value) {
            let share = share?;
            share.check(self)?;
            return Ok(DirectoryResolved { cwd: share.path });
        }
        let location = self.prompt_location(&value, context)?;
        self.validate_existing_directory_ancestor(&location)?;
        let (base, relative, absolute) = self.base(&location.path)?;
        paths::descend(base, &relative)?;
        self.authorized(false)?;
        Ok(DirectoryResolved {
            cwd: paths::display(&normalized(&absolute))?,
        })
    }
    fn validate_existing_directory_ancestor(&self, location: &Location) -> Result<()> {
        let mut candidate = PathBuf::from(&location.path);
        loop {
            let path = if candidate.as_os_str().is_empty() {
                ".".to_owned()
            } else {
                paths::display(&candidate)?
            };
            match self.stat(&Location { path }) {
                Ok(metadata) if metadata.kind == crate::EntryKind::Directory => return Ok(()),
                Ok(metadata) if metadata.kind == crate::EntryKind::Symlink => {
                    return Err(FileError::denied())
                }
                Ok(_) => {
                    return Err(FileError::new(
                        ErrorCode::Validation,
                        "Expected an accessible directory, not a file.",
                    ))
                }
                Err(error) if error.code == ErrorCode::NotFound => {
                    if !candidate.pop() {
                        return Err(error);
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
    pub fn create_directory_with_context(
        &self,
        input: &Location,
        context: Option<&FileContext>,
    ) -> Result<DirectoryResolved> {
        self.require_path_resolution()?;
        let value = unquote(&input.path)?;
        if let Some(share) = SharePath::parse(&value) {
            let share = share?;
            share.create(self)?;
            return Ok(DirectoryResolved { cwd: share.path });
        }
        let location = self.prompt_location(&value, context)?;
        self.validate_existing_directory_ancestor(&location)?;
        self.create_directory(&location)?;
        self.resolve_directory(&location)
    }
    pub fn complete_directory(&self, input: &Location) -> Result<DirectoryCompletions> {
        self.complete_directory_with_context(input, None)
    }
    pub fn complete_directory_with_context(
        &self,
        input: &Location,
        context: Option<&FileContext>,
    ) -> Result<DirectoryCompletions> {
        self.require_path_resolution()?;
        let value = unquote(&input.path)?;
        if !value.is_empty() && SharePath::parse(&value).is_none() {
            paths::validate(&value)?;
        }
        let split = value
            .char_indices()
            .rev()
            .find(|(_, character)| *character == '/' || cfg!(windows) && *character == '\\');
        let separator = split.map_or(std::path::MAIN_SEPARATOR, |(_, character)| character);
        let special = matches!(value.as_str(), "~" | ".")
            || Path::new(&value).is_absolute() && Path::new(&value).file_name().is_none();
        let (directory, prefix, fragment) = if special {
            let prefix = if value.ends_with(separator) {
                value.clone()
            } else {
                format!("{value}{separator}")
            };
            (value.clone(), prefix, String::new())
        } else {
            let index = split.map_or(0, |(index, _)| index + 1);
            (
                value[..index].to_owned(),
                value[..index].to_owned(),
                value[index..].to_owned(),
            )
        };
        let mut candidates = Candidates::new(&fragment);
        if let Some(share) = SharePath::parse(&directory) {
            share?.complete(self, &mut candidates)?;
        } else {
            let location = self.prompt_location(&directory, context)?;
            let (base, relative, _) = self.base(&location.path)?;
            for entry in paths::descend(base, &relative)?.entries()? {
                let entry = entry?;
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if !candidates.wants(&name) {
                    continue;
                }
                let metadata = entry.metadata()?;
                if metadata.is_dir() && paths::reject_link(&metadata).is_ok() {
                    candidates.keep(name);
                }
            }
        }
        let mut matches = Vec::new();
        let mut bytes = 0;
        for name in candidates.names {
            let completion = format!("{prefix}{name}{separator}");
            if completion.len() > 32767 {
                continue;
            }
            if bytes + completion.len() > DIRECTORY_COMPLETION_BYTES {
                break;
            }
            bytes += completion.len();
            matches.push(completion);
        }
        self.authorized(false)?;
        Ok(DirectoryCompletions { matches })
    }
}
