use super::*;
use anyhow::{ensure, Result};
use shellfox_core::{validation, Preferences};
use std::io::Write;
#[cfg(test)]
mod tests;

/// Only Full is safe to retry. Once the owner accepted a mutation, a lost reply
/// has an uncertain outcome and is never converted to this admission error.
pub enum NativeAdmissionError {
    Full(Box<NativeAction>),
    Stopped(Box<NativeAction>),
    Invalid(String),
}
impl std::fmt::Display for NativeAdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full(_) => f.write_str("Native owner queue is full; action was not accepted."),
            Self::Stopped(_) => f.write_str("Native owner stopped; action was not accepted."),
            Self::Invalid(message) => write!(f, "Invalid native action: {message}"),
        }
    }
}
impl std::fmt::Debug for NativeAdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for NativeAdmissionError {}
impl NativeAdmissionError {
    pub fn retry(self) -> Option<NativeAction> {
        match self {
            Self::Full(action) => Some(*action),
            _ => None,
        }
    }
}
struct BoundedSize(usize);
impl Write for BoundedSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > 1024 * 1024 {
            return Err(std::io::Error::other("metadata exceeds 1 MiB"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl NativeAction {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Metadata(request) => validate_metadata(request)?,
            Self::SetVisibleSessions(report) => shellfox_protocol::Validate::validate(report)?,
            Self::SaveWindowState(state) => {
                ensure!(state.valid(), "Invalid desktop window bounds.")
            }
            Self::Presentation {
                attachment, action, ..
            } => {
                match attachment {
                    NativeViewAttachment::Controller(a) => attachment_valid(a)?,
                    NativeViewAttachment::Observer(a) => observer_valid(a)?,
                }
                if let NativePresentation::Scroll { delta } = action {
                    ensure!(
                        delta.unsigned_abs() <= 1_000_000_000,
                        "Native presentation scroll exceeds limit."
                    );
                }
            }
            Self::Ssh(request) => request.validate()?,
            Self::System(request) => request
                .validate()
                .map_err(|error| anyhow::anyhow!(error.message))?,
            Self::Remote(request) => request
                .validate()
                .map_err(|error| anyhow::anyhow!(error.message))?,
            Self::Settings(request) => {
                serde_json::to_writer(BoundedSize(0), request)?;
                if let shellfox_host::SettingsRequest::Save(patch) = request {
                    shellfox_core::Settings::default().patched(patch.as_ref().clone())?;
                }
            }
            Self::SessionCwd { cwd, .. } | Self::TabCwd { cwd, .. } => validation::cwd(cwd)?,
            Self::Terminal {
                attachment,
                command,
                input,
                ..
            } => {
                attachment_valid(attachment)?;
                command.validate()?;
                let is_input = matches!(
                    command,
                    NativeCommand::Key(_)
                        | NativeCommand::Keys(_)
                        | NativeCommand::Paste(_)
                        | NativeCommand::Mouse { .. }
                        | NativeCommand::Wheel { .. }
                );
                ensure!(
                    is_input == input.is_some(),
                    "Native input context is required only for input commands."
                );
                if let Some(context) = input {
                    match &context.captured_settlement {
                        shellfox_host::CapturedSettlement::Captured(Some(marker)) => {
                            validation::timestamp(marker)?
                        }
                        shellfox_host::CapturedSettlement::Captured(None)
                        | shellfox_host::CapturedSettlement::Unavailable => {}
                        shellfox_host::CapturedSettlement::Legacy => {
                            anyhow::bail!("Native input cannot use a legacy settlement context.")
                        }
                    }
                }
            }
            Self::Detach { attachment, .. } | Self::Frame { attachment, .. } => {
                attachment_valid(attachment)?
            }
            Self::AcceptFrame {
                attachment,
                frame_id,
                ..
            } => {
                attachment_valid(attachment)?;
                ensure!(*frame_id > 0, "Frame ID must be nonzero.");
            }
            Self::ObserverFrame { attachment, .. }
            | Self::RecoverObserver { attachment, .. }
            | Self::Unobserve { attachment, .. } => observer_valid(attachment)?,
            Self::AcceptObserverFrame {
                attachment,
                frame_id,
                ..
            } => {
                observer_valid(attachment)?;
                ensure!(*frame_id > 0, "Frame ID must be nonzero.");
            }
            _ => {}
        }
        Ok(())
    }
}
fn attachment_valid(attachment: &Attachment) -> Result<()> {
    ensure!(
        attachment.controller > 0 && attachment.epoch > 0,
        "Invalid native control attachment."
    );
    Ok(())
}
fn observer_valid(attachment: &NativeObserverAttachment) -> Result<()> {
    ensure!(
        attachment.viewer > 0 && attachment.epoch > 0,
        "Invalid native observer attachment."
    );
    Ok(())
}
fn validate_metadata(request: &HostRequest) -> Result<()> {
    // Count serialized bytes without allocating a duplicate JSON request.
    serde_json::to_writer(BoundedSize(0), request)?;
    match request {
        HostRequest::CreateSession(input) => {
            validation::cwd(&input.cwd)?;
            if let Some(title) = &input.title {
                validation::title(title)?;
            }
        }
        HostRequest::AddTab { input, .. } => {
            if let Some(cwd) = &input.cwd {
                validation::cwd(cwd)?;
            }
            if let Some(title) = &input.title {
                validation::title(title)?;
            }
        }
        HostRequest::RenameSession { title, .. } | HostRequest::RenameTab { title, .. } => {
            validation::title(title)?
        }
        HostRequest::ReorderTabs { tab_ids, .. } => {
            ensure!(tab_ids.len() <= 10_000, "Too many tabs in native reorder.");
            let unique: std::collections::HashSet<_> = tab_ids.iter().collect();
            ensure!(
                unique.len() == tab_ids.len(),
                "Native reorder contains duplicate tabs."
            );
        }
        HostRequest::SaveSettings {
            preferences,
            terminal_env,
        } => {
            Preferences::default().patched(preferences.clone())?;
            if let Some(env) = terminal_env {
                shellfox_core::validate_env(env, cfg!(windows))?;
            }
        }
        HostRequest::SetSessionEnv { env, .. } | HostRequest::SetTerminalEnv { env } => {
            shellfox_core::validate_env(env, cfg!(windows))?
        }
        _ => {}
    }
    Ok(())
}
impl NativeCommand {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Key(event) => {
                ensure!(
                    event.text.len() <= shellfox_runtime::MAX_INPUT_BYTES
                        && event.validate().is_ok(),
                    "Invalid native key event."
                );
            }
            Self::Keys(events) => {
                ensure!(
                    (1..=shellfox_runtime::MAX_KEY_BATCH).contains(&events.len())
                        && events.iter().map(|event| event.text.len()).sum::<usize>()
                            <= shellfox_runtime::MAX_INPUT_BYTES
                        && events.iter().all(|event| event.validate().is_ok()),
                    "Invalid native key batch."
                );
            }
            Self::Paste(text) => ensure!(
                text.len() <= shellfox_runtime::MAX_INPUT_BYTES,
                "Native paste exceeds 64 KiB."
            ),
            Self::Wheel {
                event,
                geometry,
                steps,
            } => {
                ensure!(
                    shellfox_runtime::wheel_press(event)
                        && (1..=shellfox_runtime::MAX_WHEEL_BATCH).contains(steps),
                    "Invalid native wheel batch."
                );
                Self::Mouse {
                    event: *event,
                    geometry: *geometry,
                }
                .validate()?;
            }
            Self::Mouse { event, geometry } => {
                ensure!(
                    event.x.is_finite()
                        && event.y.is_finite()
                        && event.x.abs() <= 1e9
                        && event.y.abs() <= 1e9,
                    "Invalid native mouse coordinates."
                );
                ensure!(
                    (1..=1_000_000_000).contains(&geometry.cell_width)
                        && (1..=1_000_000_000).contains(&geometry.cell_height),
                    "Invalid native cell geometry."
                );
            }
            Self::Resize { cols, rows } => {
                shellfox_runtime::GridLimits::default().validate_size(*cols, *rows)?
            }
            Self::Scroll(delta) => ensure!(
                delta.unsigned_abs() <= 1_000_000_000,
                "Native scroll exceeds limit."
            ),
            _ => {} // Colors are fixed-size bytes; selection coordinates are u16.
        }
        Ok(())
    }
}
