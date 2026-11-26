//! SSH profiles and the attached desktop's window file live in the data
//! directory, outside the metadata store, and the system settings install
//! files elsewhere. Their reads and writes run on one worker thread, in
//! order, so a stalled disk delays only their replies, never the service owner.
use crate::{
    local::DesktopReply,
    native::{NativeAction, NativeResponse},
    system::{NativeSystemFiles, SystemFiles},
    Service,
};
use anyhow::{Context, Result};
use shellfox_core::window_state::WindowState;
use shellfox_protocol::CancellationToken;
use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
    thread::JoinHandle,
};
use tokio::sync::oneshot;

type Job = Box<dyn FnOnce() + Send>;

/// Started on first use; joined on drop, so no write outlives the Service
/// and the data directory it owns.
#[derive(Default)]
pub(crate) struct DeviceFiles {
    queue: Option<mpsc::Sender<Job>>,
    worker: Option<JoinHandle<()>>,
}
impl DeviceFiles {
    fn run(&mut self, job: Job) -> Result<()> {
        if self.queue.is_none() {
            let (queue, jobs) = mpsc::channel::<Job>();
            self.worker = Some(
                std::thread::Builder::new()
                    .name("shellfox-device-files".into())
                    .spawn(move || {
                        for job in jobs {
                            job();
                        }
                    })
                    .context("Device file worker unavailable.")?,
            );
            self.queue = Some(queue);
        }
        self.queue
            .as_ref()
            .context("Device file worker unavailable.")?
            .send(job)
            .map_err(|_| anyhow::anyhow!("Device file worker stopped."))
    }
}
impl Drop for DeviceFiles {
    fn drop(&mut self) {
        self.queue.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Where a device file action answers.
pub(crate) enum DeviceReply {
    Native(oneshot::Sender<Result<NativeResponse>>),
    Desktop(oneshot::Sender<Result<serde_json::Value>>),
}
impl DeviceReply {
    fn send(self, result: Result<NativeResponse>) {
        match self {
            Self::Native(reply) => {
                let _ = reply.send(result);
            }
            Self::Desktop(reply) => {
                let value = result
                    .and_then(DesktopReply::try_from)
                    .and_then(|reply| Ok(serde_json::to_value(reply)?));
                let _ = reply.send(value);
            }
        }
    }
}

impl Service {
    /// Actions that read or write a device file. The attached desktop keeps
    /// its window bounds in a file; other clients keep them in the store.
    pub(crate) fn uses_device_files(action: &NativeAction, attached: bool) -> bool {
        matches!(action, NativeAction::Ssh(_) | NativeAction::System(_))
            || attached
                && matches!(
                    action,
                    NativeAction::ReadWindowState | NativeAction::SaveWindowState(_)
                )
    }
    /// Runs a device file action on the worker; `reply` answers from there.
    /// `cancel` is the request's own grant recipient.
    pub(crate) fn enqueue_device_files(
        &mut self,
        action: NativeAction,
        controller: u64,
        cancel: CancellationToken,
        reply: DeviceReply,
    ) {
        let job = match self.device_file_job(action, controller, cancel) {
            Ok(job) => job,
            Err(error) => return reply.send(Err(error)),
        };
        // A refused job drops its reply, which its caller sees as lost.
        if let Err(error) = self.device_files.run(Box::new(move || reply.send(job()))) {
            eprintln!("Shellfox host: {error:#}");
        }
    }
    fn device_file_job(
        &self,
        action: NativeAction,
        controller: u64,
        cancel: CancellationToken,
    ) -> Result<Box<dyn FnOnce() -> Result<NativeResponse> + Send>> {
        action.validate()?;
        let window = || self.directory.join("desktop-window.json");
        match action {
            NativeAction::Ssh(request) => {
                self.assert_accepting()?;
                let store: Arc<crate::ssh::Store> = self
                    .ssh_profiles
                    .clone()
                    .context("SSH profile storage is unavailable.")?;
                let lost = self.ownership_lost.clone();
                Ok(Box::new(move || {
                    // The write re-checks its grant right before it commits.
                    let check = || {
                        if cancel.is_cancelled() || lost.is_cancelled() {
                            Err(shellfox_ssh::Error::Permissions)
                        } else {
                            Ok(())
                        }
                    };
                    store.request(request, check).map(NativeResponse::Ssh)
                }))
            }
            NativeAction::System(request) => {
                let job = self
                    .system_native_job(request, controller)
                    .map_err(|error| anyhow::anyhow!(error.message))?;
                Ok(Box::new(move || {
                    job()
                        .map(NativeResponse::System)
                        .map_err(|error| anyhow::anyhow!(error.message))
                }))
            }
            NativeAction::ReadWindowState => {
                self.assert_owned()?;
                let path = window();
                Ok(Box::new(move || {
                    let file = NativeSystemFiles
                        .read(&path)
                        .map_err(|error| anyhow::anyhow!(error.message))?;
                    Ok(NativeResponse::WindowState(
                        file.as_ref()
                            .filter(|file| file.bytes.len() < 4096)
                            .and_then(|file| WindowState::parse(&file.bytes)),
                    ))
                }))
            }
            NativeAction::SaveWindowState(state) => {
                self.assert_accepting()?;
                let path: PathBuf = window();
                Ok(Box::new(move || {
                    let file = NativeSystemFiles
                        .read(&path)
                        .map_err(|error| anyhow::anyhow!(error.message))?;
                    NativeSystemFiles
                        .replace(
                            &path,
                            file.as_ref(),
                            Some(&serde_json::to_vec(&state)?),
                            false,
                        )
                        .map_err(|error| anyhow::anyhow!(error.message))?;
                    Ok(NativeResponse::Applied)
                }))
            }
            _ => anyhow::bail!("Not a device file action."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn the_attached_window_file_is_written_and_read_off_the_owner() {
        let data = crate::tests::scratch();
        let mut service =
            Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
        let state = WindowState {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
            maximized: false,
            display: None,
        };
        let save = NativeAction::SaveWindowState(state.clone());
        assert!(Service::uses_device_files(&save, true));
        assert!(!Service::uses_device_files(&save, false));
        let (reply, saved) = oneshot::channel();
        service.enqueue_device_files(
            save,
            0,
            CancellationToken::new(),
            DeviceReply::Native(reply),
        );
        assert!(matches!(
            saved.await.unwrap().unwrap(),
            NativeResponse::Applied
        ));
        let (reply, read) = oneshot::channel();
        let action = NativeAction::ReadWindowState;
        service.enqueue_device_files(
            action,
            0,
            CancellationToken::new(),
            DeviceReply::Native(reply),
        );
        assert!(matches!(
            read.await.unwrap().unwrap(),
            NativeResponse::WindowState(Some(read)) if read == state
        ));
    }
}
