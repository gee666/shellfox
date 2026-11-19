//! Retained physical topology from the shared native producer, not path-string proof.
#[cfg(test)]
#[path = "frontend_composition_tests.rs"]
mod tests;
use crate::{
    frontend_retention::{FrontendRetention, PreservedFrontendOwner},
    options::Options,
};
use anyhow::{Context, Result, ensure};
use shellfox_files::{Access, DirectoryPlan, DirectoryRelation, FileService, Location};
use std::{path::Path, sync::Arc};

pub(crate) struct FrontendPlans {
    host: DirectoryPlan,
    ui: DirectoryPlan,
    device: DirectoryPlan,
    run: DirectoryPlan,
    editor: DirectoryPlan,
    files: Arc<FileService>,
}
impl FrontendPlans {
    pub fn revalidate(&self) -> Result<()> {
        for plan in [&self.host, &self.ui, &self.device, &self.run, &self.editor] {
            plan.revalidate()?;
        }
        for (left, right) in [
            (&self.host, &self.ui),
            (&self.host, &self.device),
            (&self.ui, &self.device),
            (&self.host, &self.run),
            (&self.host, &self.editor),
            (&self.device, &self.run),
            (&self.device, &self.editor),
            (&self.run, &self.editor),
        ] {
            ensure!(
                left.relation_to(right)? == DirectoryRelation::Disjoint,
                "Frontend namespaces overlap or physical separation is unproven"
            );
        }
        ensure!(
            self.ui.relation_to(&self.run)? == DirectoryRelation::Ancestor,
            "Run must be a proven strict UI descendant"
        );
        ensure!(
            self.ui.relation_to(&self.editor)? == DirectoryRelation::Ancestor,
            "Editor must be a proven strict UI descendant"
        );
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum SensitiveRoot {
    Ui,
    Run,
    Device,
    Editor,
}
pub(crate) struct FrontendDirectories {
    plans: FrontendPlans,
    retention: FrontendRetention,
    failed: bool,
    ui: Option<Arc<FileService>>,
    run: Option<Arc<FileService>>,
    device: Option<Arc<FileService>>,
    editor: Option<Arc<shellfox_files::EditorBackend>>,
}
impl FrontendDirectories {
    /// Read-only construction. Keep this original issuer outside catch_unwind BEFORE prepare.
    pub fn plan(options: &Options) -> Result<Self> {
        let plans = inspect(options)?;
        let retention = FrontendRetention::new(&plans.files);
        Ok(Self {
            plans,
            retention,
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
            !self.failed && !self.retention.is_failed(),
            "Frontend issuer already failed; no creation retry is admitted"
        );
        self.revalidate()
            .context("Validate frontend topology before private creation")?;
        let plan = match root {
            SensitiveRoot::Ui => &self.plans.ui,
            SensitiveRoot::Run => &self.plans.run,
            SensitiveRoot::Device => &self.plans.device,
            SensitiveRoot::Editor => &self.plans.editor,
        };
        plan.repair_app_directory()
            .context("Cannot make the Shellfox frontend state directory private")?;
        let cap = match plan.create_private_approved_root() {
            Ok(cap) => cap,
            Err(failure) => {
                self.failed = true;
                return Err(self
                    .retention
                    .capture_directory_failure(failure)
                    .context("Native private root creation/verification"));
            }
        };
        if let Err(error) = self.revalidate() {
            self.failed = true;
            return Err(error.context("Validate frontend topology after private creation"));
        }
        Ok(cap)
    }
    pub fn revalidate(&self) -> Result<()> {
        self.plans.revalidate()
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
    /// Metadata scope and document authority are separate. The original planning issuer already
    /// grants local account RW; journal privacy does not mint that document authority.
    pub fn prepare_editor(&mut self) -> Result<Arc<shellfox_files::EditorBackend>> {
        ensure!(
            !self.failed && !self.retention.is_failed(),
            "Frontend preparation failed; no retry is admitted"
        );
        self.revalidate()?;
        if let Some(editor) = &self.editor {
            return Ok(editor.clone());
        }
        let metadata = self.create_private(SensitiveRoot::Editor)?;
        match shellfox_files::EditorBackend::new_anchored(self.plans.files.clone(), metadata) {
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
    /// Explicit disposition, retaining the same keeper and parent/evidence in the returned type.
    pub fn preserve(self) -> PreservedFrontendOwner {
        self.retention.preserve()
    }
}
#[cfg(test)]
pub(crate) fn prepare(options: &Options) -> Result<FrontendDirectories> {
    let mut frontend = FrontendDirectories::plan(options)?;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| frontend.prepare()));
    match outcome {
        Ok(Ok(())) => Ok(frontend),
        Ok(Err(error)) => Err(anyhow::Error::new(frontend.preserve().with_error(error))),
        Err(_) => {
            Err(anyhow::Error::new(frontend.preserve().with_error(
                anyhow::anyhow!("Test frontend preparation panicked"),
            )))
        }
    }
}
fn location(path: &Path) -> Result<Location> {
    Ok(Location {
        path: path.to_str().context("Frontend root must be UTF-8")?.into(),
    })
}
pub(crate) fn inspect(options: &Options) -> Result<FrontendPlans> {
    let files = Arc::new(FileService::local_desktop(Access::ReadWrite));
    let plans = FrontendPlans {
        host: files.plan_directory(&location(&options.data_dir)?)?,
        ui: files.plan_directory(&location(&options.desktop_dir)?)?,
        device: files.plan_directory(&location(&options.device_config_root)?)?,
        run: files.plan_directory(&location(&options.run_dir)?)?,
        editor: files.plan_directory(&location(&options.desktop_dir.join("file-editor"))?)?,
        files,
    };
    plans.revalidate()?;
    Ok(plans)
}
