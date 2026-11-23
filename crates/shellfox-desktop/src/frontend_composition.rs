//! The frontend's private state directories: UI scratch, per-window run and device config.
//! `Options::parse` keeps them apart from the host root; `FileService::private_root`
//! creates them and refuses links, junctions and name aliases.
#[cfg(test)]
mod tests;
use crate::options::Options;
use anyhow::{Context, Result};
use shellfox_files::{Access, EditorBackend, FileService};
use std::{path::PathBuf, sync::Arc};

pub(crate) struct FrontendDirectories {
    ui: PathBuf,
    run: PathBuf,
    device: PathBuf,
    device_root: Option<Arc<FileService>>,
    editor: Option<Arc<EditorBackend>>,
}

impl FrontendDirectories {
    /// Checks the configured paths. Creates nothing.
    pub fn plan(options: &Options) -> Result<Self> {
        inspect(options)?;
        Ok(Self {
            ui: options.desktop_dir.clone(),
            run: options.run_dir.clone(),
            device: options.device_config_root.clone(),
            device_root: None,
            editor: None,
        })
    }

    pub fn prepare(&mut self) -> Result<()> {
        private(&self.ui).context("Create private frontend UI root")?;
        private(&self.run).context("Create private frontend run root")?;
        Ok(())
    }

    pub fn device_root(&mut self) -> Result<Arc<FileService>> {
        if let Some(root) = &self.device_root {
            return Ok(root.clone());
        }
        let root = private(&self.device).context("Create private frontend device root")?;
        self.device_root = Some(root.clone());
        Ok(root)
    }

    /// The editor works with the local account document authority.
    pub fn prepare_editor(&mut self) -> Result<Arc<EditorBackend>> {
        if let Some(editor) = &self.editor {
            return Ok(editor.clone());
        }
        let files = Arc::new(FileService::local_desktop(Access::ReadWrite));
        let editor = Arc::new(EditorBackend::new(files)?);
        self.editor = Some(editor.clone());
        Ok(editor)
    }
}

fn private(path: &std::path::Path) -> Result<Arc<FileService>> {
    FileService::private_root(path).map_err(|error| {
        anyhow::Error::new(error).context("Cannot prepare a private Shellfox frontend directory")
    })
}

#[cfg(test)]
pub(crate) fn prepare(options: &Options) -> Result<FrontendDirectories> {
    let mut frontend = FrontendDirectories::plan(options)?;
    frontend.prepare()?;
    Ok(frontend)
}

/// Each frontend path must be UTF-8 and free of links and aliases up to its existing prefix.
pub(crate) fn inspect(options: &Options) -> Result<()> {
    for path in [
        &options.desktop_dir,
        &options.device_config_root,
        &options.run_dir,
    ] {
        path.to_str().context("Frontend root must be UTF-8")?;
        FileService::check_private_root(path)
            .with_context(|| format!("Unsafe frontend directory: {}", path.display()))?;
    }
    Ok(())
}
