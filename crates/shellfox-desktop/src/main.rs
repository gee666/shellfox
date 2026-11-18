mod accessibility;
mod app;
pub mod app_host;
mod app_launch;
mod app_startup;
mod assets;
mod brand;
mod chrome;
mod composition;
mod desktop_windows;
mod device_prefs;
mod dialogs;
mod directory;
mod directory_view;
mod editor_composition;
mod field;
mod files;
mod files_navigation;
mod files_view;
mod frontend_composition;
mod frontend_retention;
mod host_client;
mod input;
mod launch_handoff;
mod launch_runtime;
mod layout;
mod options;
mod profile_menu;
mod renderer;
mod root;
mod root_ssh;
mod session_menu;
mod settings;
mod sidebar;
mod smoke;
#[cfg(test)]
mod startup_dispatch_tests;
mod terminal_semantics;
mod theme;
pub mod window_manager;
mod workspace;

use anyhow::Result;
use gpui::{App, Application};
use options::{Mode, Options};

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(code) = shellfox_service::ssh::dispatch_early_entry(&arguments)? {
        std::process::exit(i32::from_ne_bytes(code.to_ne_bytes()));
    }
    let options = Options::parse(arguments)?;
    if options.mode == Mode::Smoke {
        let mut frontend = frontend_composition::FrontendDirectories::plan(&options)?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            frontend.prepare()?;
            smoke::run(&options.run_dir)
        }));
        return finish_frontend(frontend.preserve(), result);
    }
    let app_startup::Startup::Graphical(owner) = app_startup::prepare(&options)? else {
        return Ok(());
    };
    if let Some(reason) = owner.as_ref().and_then(|owner| owner.remote_unavailable) {
        match reason {
            shellfox_service::remote::RemoteUnavailableReason::NativeApprovalStoragePending => {
                eprintln!(
                    "Remote unavailable: native approval storage review is pending; continuing with the same local host."
                );
            }
        }
    }
    // This exact owner stays outside Application.run, including install/GPU failure.
    eprintln!(
        "Experiment scratch directory: {}",
        options.run_dir.display()
    );
    let mut frontend = match frontend_composition::FrontendDirectories::plan(&options) {
        Ok(frontend) => frontend,
        Err(error) => {
            if let Some(owner) = &owner {
                owner.cleanup_until_confirmed();
            }
            return Err(error);
        }
    };
    // Original issuer and panic keeper are OUTSIDE the unwind/error scope.
    // Logging only. Unlike --ui-acceptance this does not change presentation.
    let trace = options.ui_acceptance
        || std::env::var_os("SHELLFOX_UI_INIT_TRACE").is_some_and(|value| value == "1");
    let graphics = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        trace_ui_init(trace, "frontend.prepare begin");
        frontend.prepare()?;
        trace_ui_init(trace, "frontend.prepare returned");
        trace_ui_init(trace, "device.prepare begin");
        let device_colors = device_prefs::prepare(&options, &mut frontend)?;
        trace_ui_init(trace, "device.prepare returned");
        trace_ui_init(trace, "editor.prepare begin");
        let editors = editor_composition::prepare(&mut frontend);
        trace_ui_init(
            trace,
            match &editors {
                editor_composition::EditorOutcome::Ready(_) => "editor.prepare returned Ready",
                editor_composition::EditorOutcome::Unavailable(_) => {
                    "editor.prepare returned Unavailable"
                }
            },
        );
        run_graphics(options, owner.as_ref(), device_colors, editors, trace)
    }));
    let preserved = frontend.preserve();
    if let Some(owner) = &owner {
        owner.cleanup_until_confirmed();
    }
    finish_frontend(preserved, graphics)
}

fn finish_frontend(
    preserved: frontend_retention::PreservedFrontendOwner,
    outcome: std::thread::Result<Result<()>>,
) -> Result<()> {
    match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(anyhow::Error::new(preserved.with_error(error))),
        Err(_) => Err(anyhow::Error::new(
            preserved.with_error(anyhow::anyhow!("Frontend execution panicked")),
        )),
    }
}

fn trace_ui_init(enabled: bool, stage: &str) {
    if enabled {
        eprintln!("UI init stage: {stage}");
    }
}

fn run_graphics(
    options: Options,
    owner: Option<&app_startup::Prestarted>,
    device_colors: std::sync::Arc<settings::DeviceColors>,
    editors: editor_composition::EditorOutcome,
    trace: bool,
) -> Result<()> {
    let retained = owner.map(|owner| (owner.config.clone(), owner.host.clone()));
    let errors = app_startup::InitErrors(std::rc::Rc::new(std::cell::RefCell::new(None)));
    let app_errors = errors.clone();
    trace_ui_init(trace, "run_graphics entered; Application::new begin");
    let application = Application::new();
    trace_ui_init(trace, "Application::new returned");
    application
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            trace_ui_init(trace, "Application run callback entered");
            cx.set_global(app_errors);
            cx.set_global(device_prefs::DevicePrefs(device_colors));
            cx.set_global(editors);
            trace_ui_init(trace, "prestarted adapter install begin");
            if let Some((config, host)) = retained
                && let Err(error) = app_host::adapter::install_prestarted(cx, config, &host)
            {
                app_startup::record_init_failure(error, cx);
                cx.quit();
                return;
            }
            trace_ui_init(trace, "prestarted adapter install returned");
            // Quitting the event loop does not drop the sole cleanup owner. Main
            // drives its retained endpoint to actual confirmed Stop before returning.
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            launch_runtime::set_template(options.clone(), cx);
            let initial = if matches!(&options.launch.action, app_launch::LaunchAction::Show {}) {
                window_manager::InitialSelection::Recent
            } else {
                window_manager::InitialSelection::Empty
            };
            trace_ui_init(trace, "desktop_windows::open begin");
            match desktop_windows::open(options, initial, cx) {
                Ok(window) => {
                    trace_ui_init(trace, "desktop_windows::open returned Ok");
                    launch_runtime::set_bootstrap(window, cx);
                }
                Err(error) => {
                    trace_ui_init(trace, "desktop_windows::open returned Err");
                    app_startup::record_init_failure(error, cx);
                    cx.quit();
                }
            }
        });
    trace_ui_init(trace, "Application run returned");
    let error = errors.0.borrow_mut().take();
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
