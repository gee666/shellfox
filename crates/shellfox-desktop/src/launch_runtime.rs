//! One app-owned in-process launch registration/coordinator. No wire registration or second Host.
use crate::{
    app_launch::{
        AppLaunchCoordinator, LaunchDeliveryClient, LaunchDisposition, LaunchError, LaunchFuture,
        LaunchWindowTransport,
    },
    desktop_windows,
    host_client::HostClient,
    options::{Mode, Options},
    root::Root,
    window_manager::InitialSelection,
};
use gpui::{App, AsyncApp, Global, Task, WindowHandle};
use shellfox_core::SessionId;
use shellfox_service::{
    desktop_launch::{
        DesktopLaunchDelivery, LaunchClaim, LaunchHandling, LaunchReceipt, LaunchRequestKind,
        LaunchTarget,
    },
    native::{NativeAction, NativeResponse},
};
use std::time::{Duration, Instant};

const RETRY: Duration = Duration::from_millis(50);
mod unregister;

struct LaunchGlobal {
    options: Options,
    bootstrap: Option<WindowHandle<Root>>,
    installed: bool,
    task: Option<Task<()>>,
}
impl Global for LaunchGlobal {}
pub(crate) fn set_template(options: Options, cx: &mut App) {
    cx.set_global(LaunchGlobal {
        options,
        bootstrap: None,
        installed: false,
        task: None,
    });
}
pub(crate) fn set_bootstrap(window: WindowHandle<Root>, cx: &mut App) {
    cx.global_mut::<LaunchGlobal>().bootstrap = Some(window);
}
fn error(error: anyhow::Error) -> LaunchError {
    error
        .downcast_ref::<LaunchError>()
        .copied()
        .unwrap_or(LaunchError::Unavailable)
}
struct DeliveryClient(HostClient);
impl LaunchDeliveryClient for DeliveryClient {
    fn claim<'a>(&'a self, delivery: &'a DesktopLaunchDelivery) -> LaunchFuture<'a, LaunchClaim> {
        Box::pin(async move { self.0.desktop_claim(delivery.clone()).await.map_err(error) })
    }
    fn complete<'a>(
        &'a self,
        delivery: &'a DesktopLaunchDelivery,
        handling: LaunchHandling,
    ) -> LaunchFuture<'a, LaunchReceipt> {
        Box::pin(async move {
            self.0
                .desktop_complete(delivery.clone(), handling)
                .await
                .map_err(error)
        })
    }
}

/// Called after the first Root receives its client. The Global owns the task;
/// later Roots neither register again nor own the coordinator's lifetime.
pub(crate) fn install(cx: &mut App) {
    if !cx.has_global::<LaunchGlobal>() || cx.global::<LaunchGlobal>().installed {
        return;
    }
    let options = cx.global::<LaunchGlobal>().options.clone();
    if options.mode != Mode::Shell {
        return;
    }
    let Some(original_host) = crate::app_host::adapter::get(cx) else {
        eprintln!("Launch coordinator requires the installed original application owner.");
        return;
    };
    cx.global_mut::<LaunchGlobal>().installed = true;
    let task = cx.spawn(async move |cx| {
        let client = match original_host.chrome_client().await {
            Ok(client) => client,
            Err(error) => {
                eprintln!("Desktop launch client unavailable: {error:#}");
                return;
            }
        };
        let delivery = DeliveryClient(client);
        let registration = match delivery.0.desktop_register().await {
            Ok(registration) => registration,
            Err(error) => {
                eprintln!("Desktop launch registration unavailable; no retry: {error:#}");
                return;
            }
        };
        let owner = registration.token.owner();
        // Only now is this window the host's desktop: a UI that reattaches after a
        // crash also stops this host on Quit. A second launch meanwhile hands off.
        if !original_host.is_external()
            && let Err(error) = crate::host_process::record_owner(&options.desktop_dir, owner)
        {
            eprintln!("Shellfox host ownership not recorded: {error:#}");
        }
        let token = registration.token;
        let Some(bootstrap) = cx
            .update(|cx| cx.global::<LaunchGlobal>().bootstrap)
            .ok()
            .flatten()
        else {
            eprintln!("Bootstrap window is unavailable; no initial launch was submitted");
            if let Err(error) = unregister::finish(&delivery.0, token, &original_host).await {
                eprintln!("Desktop launch unregister was not confirmed: {error:#}");
            }
            return;
        };
        let mut coordinator = AppLaunchCoordinator::new(registration);
        // Metadata creation/deduplication belongs to the same existing Service router.
        // Queued admission is not a handled receipt; only pump/complete acknowledges UI application.
        if let Err(error) = delivery
            .0
            .desktop_submit(owner, options.launch.clone(), LaunchRequestKind::Fresh)
            .await
        {
            eprintln!("Initial desktop launch was not confirmed; no replay: {error:#}");
        }
        let mut windows = Windows {
            app: cx.clone(),
            options: options.clone(),
            bootstrap: Some((options.launch.request_id, bootstrap)),
        };
        loop {
            // Deliveries arrive through the host's held PollAttached reads.
            if coordinator.ready().await.is_err() {
                break;
            }
            let retry = match coordinator.pump(&delivery, &mut windows).await {
                Ok(_) => false,
                Err(LaunchError::StaleOwner | LaunchError::StaleClient) => break,
                Err(error) => {
                    eprintln!("Desktop launch delivery: {error}");
                    true
                }
            };
            if cx
                .update(|cx| {
                    crate::app_host::adapter::get(cx)
                        .is_none_or(|host| host.is_stopped().unwrap_or(false))
                })
                .unwrap_or(true)
            {
                break;
            }
            if retry {
                // A busy window or host; try the same delivery again shortly.
                cx.background_executor().timer(RETRY).await;
            }
        }
        if let Err(error) = unregister::finish(&delivery.0, token, &original_host).await {
            eprintln!("Desktop launch unregister was not confirmed: {error:#}");
        }
    });
    cx.global_mut::<LaunchGlobal>().task = Some(task);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    Bootstrap,
    New,
    Existing,
}
fn destination(
    startup: Option<SessionId>,
    request: SessionId,
    disposition: LaunchDisposition,
) -> Destination {
    if startup == Some(request) {
        Destination::Bootstrap
    } else if disposition == LaunchDisposition::NewWindow {
        Destination::New
    } else {
        Destination::Existing
    }
}

struct Windows {
    app: AsyncApp,
    options: Options,
    bootstrap: Option<(SessionId, WindowHandle<Root>)>,
}
impl Windows {
    fn existing(&self) -> Result<WindowHandle<Root>, LaunchError> {
        self.app
            .update(|cx| {
                cx.active_window()
                    .and_then(|handle| handle.downcast::<Root>())
                    .or_else(|| {
                        cx.windows()
                            .into_iter()
                            .find_map(|handle| handle.downcast::<Root>())
                    })
            })
            .map_err(error)?
            .ok_or(LaunchError::Unavailable)
    }
    async fn ready(
        &mut self,
        window: WindowHandle<Root>,
    ) -> Result<(HostClient, u64), LaunchError> {
        let deadline = Instant::now() + Duration::from_secs(15);
        // Re-check whenever this window's Root changes; the deadline bounds the wait.
        let (notify, mut changed) = tokio::sync::watch::channel(());
        let root = window
            .update(&mut self.app, |_, _, cx| cx.entity())
            .map_err(error)?;
        let _subscription = self
            .app
            .update(|cx| cx.observe(&root, move |_, _| notify.send_replace(())))
            .map_err(error)?;
        loop {
            let state = window
                .read_with(&self.app, |root, _| {
                    if root.close.closing || root.close.check_pending {
                        return Err(LaunchError::Busy);
                    }
                    if root.host.is_none() && root.error.is_some() {
                        return Err(LaunchError::Unavailable);
                    }
                    if root.dialogs.user_action_pending || root.dialogs.current.is_some() {
                        return Ok(None);
                    }
                    Ok(root
                        .host
                        .as_ref()
                        .filter(|_| root.snapshot.is_some())
                        .map(|client| (client.clone(), root.workspace.navigation.epoch())))
                })
                .map_err(error)??;
            if let Some(state) = state {
                return Ok(state);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(LaunchError::DeliveryUncertain);
            }
            let timeout = self.app.background_executor().timer(remaining);
            tokio::select! {
                _ = changed.changed() => {}
                () = timeout => {}
            }
        }
    }
}
impl LaunchWindowTransport for Windows {
    fn apply<'a>(
        &'a mut self,
        request_id: SessionId,
        target: &'a LaunchTarget,
        disposition: LaunchDisposition,
    ) -> LaunchFuture<'a, ()> {
        Box::pin(async move {
            let window =
                match destination(self.bootstrap.map(|(id, _)| id), request_id, disposition) {
                    Destination::Bootstrap => self
                        .bootstrap
                        .take()
                        .map(|(_, window)| window)
                        .ok_or(LaunchError::Unavailable)?,
                    Destination::New => {
                        let options = self.options.for_window().map_err(error)?;
                        let options = self
                            .app
                            .background_executor()
                            .spawn(async move {
                                // The executor does not catch panics; an uncaught one would hang this launch.
                                std::panic::catch_unwind(|| {
                                    crate::frontend_composition::FrontendDirectories::plan(
                                        &options,
                                    )?
                                    .prepare()
                                })
                                .unwrap_or_else(|_| {
                                    Err(anyhow::anyhow!("New-window frontend preparation panicked"))
                                })
                                .map(|()| options)
                            })
                            .await
                            .map_err(error)?;
                        self.app
                            .update(|cx| {
                                desktop_windows::open(options, InitialSelection::Empty, cx)
                            })
                            .map_err(error)?
                            .map_err(error)?
                    }
                    Destination::Existing => self.existing()?,
                };
            let (client, epoch) = self.ready(window).await?;
            let NativeResponse::Snapshot(snapshot) = client
                .execute(NativeAction::Metadata(
                    shellfox_host::HostRequest::GetSnapshot {},
                ))
                .await
                .map_err(error)?
            else {
                return Err(LaunchError::Invalid);
            };
            window
                .update(&mut self.app, |root, window, cx| {
                    root.apply_launch_target(
                        snapshot,
                        target,
                        epoch,
                        !self.options.ui_acceptance,
                        window,
                        cx,
                    )
                })
                .map_err(error)?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_new_window_request_selects_bootstrap_destination() {
        let initial = SessionId::new();
        let external = SessionId::new();
        assert_eq!(
            destination(Some(initial), initial, LaunchDisposition::NewWindow),
            Destination::Bootstrap
        );
        assert_eq!(
            destination(Some(initial), initial, LaunchDisposition::ExistingWindow),
            Destination::Bootstrap
        );
        assert_eq!(
            destination(Some(initial), external, LaunchDisposition::NewWindow),
            Destination::New
        );
        assert_eq!(
            destination(Some(initial), external, LaunchDisposition::ExistingWindow),
            Destination::Existing
        );
        assert_eq!(
            destination(None, external, LaunchDisposition::NewWindow),
            Destination::New
        );
    }
}
