use crate::{capability_unavailable, error, file_ops, Actor};
use shellfox_files as files;
use shellfox_protocol::{self as wire, AppError, ErrorCode};
use std::path::Path;

fn unquote(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        value
    }
}
fn selection<'a>(
    actor: &'a Actor,
    value: &str,
    home: Option<&str>,
) -> Result<(&'a files::FileService, files::FileContext), AppError> {
    // Selection never grants authority. FileService repeats parsing and confines
    // the resulting path to this exact approved-root capability.
    let unquoted = unquote(value);
    let selection = if Path::new(unquoted).is_absolute() {
        unquoted
    } else {
        home.ok_or_else(|| {
            capability_unavailable("host home directory for relative directory prompt")
        })?
    };
    let capability = file_ops::capability(actor, selection)?;
    let cwd = home.unwrap_or(selection).to_owned();
    Ok((
        capability.as_ref(),
        files::FileContext {
            cwd,
            home: home.map(str::to_owned),
        },
    ))
}
pub fn resolve(
    actor: &Actor,
    input: &wire::ResolveDirectory,
    home: Option<&str>,
) -> Result<serde_json::Value, AppError> {
    validate(input)?;
    let (capability, context) = selection(actor, &input.path, home)?;
    let result = capability
        .resolve_directory_with_context(
            &files::Location {
                path: input.path.clone(),
            },
            Some(&context),
        )
        .map_err(file_ops::file_error)?;
    file_ops::checked(wire::Directory { cwd: result.cwd })
}
pub fn complete(
    actor: &Actor,
    input: &wire::CompleteDirectory,
    home: Option<&str>,
) -> Result<serde_json::Value, AppError> {
    validate(input)?;
    let (capability, context) = selection(actor, &input.path, home)?;
    let result = capability
        .complete_directory_with_context(
            &files::Location {
                path: input.path.clone(),
            },
            Some(&context),
        )
        .map_err(file_ops::file_error)?;
    file_ops::checked(wire::DirectoryMatches {
        matches: result.matches,
    })
}
pub fn create(
    actor: &Actor,
    input: &wire::CreateDirectory,
    home: Option<&str>,
) -> Result<serde_json::Value, AppError> {
    validate(input)?;
    let (capability, context) = selection(actor, &input.path, home)?;
    // Resolve host context once, then mutate only that capability. Never probe
    // another root or retry a publication through an alternate path.
    let raw = unquote(&input.path);
    let location = capability
        .resolve_location(&files::Location { path: raw.into() }, &context)
        .map_err(file_ops::file_error)?;
    capability
        .create_directory(&location)
        .map_err(file_ops::file_error)?;
    file_ops::checked(wire::Directory { cwd: location.path })
}
use wire::Validate;
fn validate<T: Validate>(input: &T) -> Result<(), AppError> {
    input
        .validate()
        .map_err(|_| error(ErrorCode::Validation, "Invalid directory request.", false))
}
