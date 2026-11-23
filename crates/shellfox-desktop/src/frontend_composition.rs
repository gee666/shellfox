//! The frontend's private state directories: UI, per-window run and device config.
//! `Options::parse` keeps them apart from the host root; `FileService::private_root`
//! refuses links, junctions and name aliases while creating them.
#[cfg(test)]
#[path = "frontend_composition_tests.rs"]
mod tests;
use crate::{frontend_retention::PreservedFrontendOwner, options::Options};
use anyhow::{Context, Result, ensure};
use shellfox_files::{Access, FileService, Location};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct FrontendPlans {
    ui: PathBuf,
    device: PathBuf,
    run: PathBuf,
    /// Document authority for the editor.
    files: Arc<FileService>,
}
#[derive(Clone, Copy)]
enum SensitiveRoot {
    Ui,
    Run,
    Device,
}
pub(crate) struct FrontendDirectories {
    plans: FrontendPlans,
    failed: bool,
    ui: Option<Arc<FileService>>,
    run: Option<Arc<FileService>>,
    device: Option<Arc<FileService>>,
    editor: Option<Arc<shellfox_files::EditorBackend>>,
}
impl FrontendDirectories {
    /// Validates the configured paths. Creates nothing.
    pub fn plan(options: &Options) -> Result<Self> {
        Ok(Self {
            plans: inspect(options)?,
            failed: false,
            ui: None,
            run: None,
            device: None,
            editor: None,
        })
    }
    pub fn prepare(&mut self) -> Result<()> {
        if self.ui.is_none() {
            self.ui = Some(
                self.create_private(SensitiveRoot::Ui)
                    .context("Create private frontend UI root")?,
            );
        }
        if self.run.is_none() {
            self.run = Some(
                self.create_private(SensitiveRoot::Run)
                    .context("Create private frontend run root")?,
            );
        }
        self.revalidate()
    }
    fn create_private(&mut self, root: SensitiveRoot) -> Result<Arc<FileService>> {
        ensure!(
            !self.failed,
            "Frontend preparation already failed; it is not retried"
        );
        let path = match root {
            SensitiveRoot::Ui => &self.plans.ui,
            SensitiveRoot::Run => &self.plans.run,
            SensitiveRoot::Device => &self.plans.device,
        };
        FileService::private_root(path).map_err(|error| {
            self.failed = true;
            anyhow::Error::new(error).context("Cannot prepare a private Shellfox frontend directory")
        })
    }
    /// Each prepared root still opens the directory it was created with.
    pub fn revalidate(&self) -> Result<()> {
        for root in [&self.ui, &self.run, &self.device]
            .into_iter()
            .flatten()
        {
            let path = root.root_path().context("Frontend root has no path")?;
            root.stat(&location(path)?)?;
        }
        Ok(())
    }
    pub fn device_root(&mut self) -> Result<Arc<FileService>> {
        self.revalidate()?;
        if self.device.is_none() {
            self.device = Some(
                self.create_private(SensitiveRoot::Device)
                    .context("Create private frontend device root")?,
            );
        }
        self.device
            .as_ref()
            .cloned()
            .context("Device capability is unavailable")
    }
    /// The editor works with the local account document authority.
    pub fn prepare_editor(&mut self) -> Result<Arc<shellfox_files::EditorBackend>> {
        ensure!(!self.failed, "Frontend preparation failed; it is not retried");
        self.revalidate()?;
        if let Some(editor) = &self.editor {
            return Ok(editor.clone());
        }
        match shellfox_files::EditorBackend::new(self.plans.files.clone()) {
            Ok(editor) => {
                let editor = Arc::new(editor);
                self.editor = Some(editor.clone());
                Ok(editor)
            }
            Err(error) => {
                self.failed = true;
                Err(error.into())
            }
        }
    }
    /// Keeps the prepared scopes open for the caller's outcome reporting.
    pub fn preserve(self) -> PreservedFrontendOwner {
        PreservedFrontendOwner::new(
            [self.ui, self.run, self.device]
                .into_iter()
                .flatten()
                .collect(),
        )
    }
}
#[cfg(test)]
pub(crate) fn prepare(options: &Options) -> Result<FrontendDirectories> {
    let mut frontend = FrontendDirectories::plan(options)?;
    frontend.prepare()?;
    Ok(frontend)
}
fn location(path: &Path) -> Result<Location> {
    Ok(Location {
        path: path.to_str().context("Frontend root must be UTF-8")?.into(),
    })
}
pub(crate) fn inspect(options: &Options) -> Result<FrontendPlans> {
    for path in [
        &options.desktop_dir,
        &options.device_config_root,
        &options.run_dir,
    ] {
        location(path)?;
        FileService::check_private_root(path)
            .with_context(|| format!("Unsafe frontend directory: {}", path.display()))?;
    }
    Ok(FrontendPlans {
        ui: options.desktop_dir.clone(),
        device: options.device_config_root.clone(),
        run: options.run_dir.clone(),
        files: Arc::new(FileService::local_desktop(Access::ReadWrite)),
    })
}
