// A GUI binary: no console window. Shells run in shellfox-cli, a console binary.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod accessibility;
mod app;
pub mod app_host;
mod app_launch;
mod app_startup;
mod assets;
mod brand;
mod chrome;
mod cli_launcher;
mod composition;
mod desktop_errors;
mod desktop_geometry;
mod startup_chrome;
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
mod host_lost;
mod host_process;
mod input;
mod launch_handoff;
mod launch_runtime;
mod layout;
mod layout_shortcuts;
mod app_menu;
mod options;
mod profile_menu;
mod renderer;
mod root;
mod session_environment;
mod session_menu;
mod session_visibility;
#[cfg(target_os = "macos")]
mod macos_window;
mod settings;
mod sidebar;
mod smoke;
#[cfg(windows)]
mod squirrel;
#[cfg(test)]
mod startup_dispatch_tests;
mod tab_activity;
mod terminal_semantics;
mod theme;
mod ui_font;
#[cfg(target_os = "linux")]
mod linux_fontconfig;
mod update;
mod update_view;
pub mod window_manager;
mod workspace;

use anyhow::Result;
use gpui::{App, Application};
use options::{Mode, Options};

fn main() -> Result<()> {
    let mut arguments: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(windows)]
    if arguments.first().is_some_and(|first| ["--version", "--smoke", "ssh"].iter().any(|flag| first == flag)) {
        attach_parent_console();
    }
    if arguments.len() == 1 && arguments[0] == "--version" {
        println!("Shellfox {}", env!("SHELLFOX_VERSION"));
        return Ok(());
    }
    // `shellfox update` on desktop installs, forwarded by shellfox-cli.
    if arguments.first().is_some_and(|first| first == update::cli::FLAG) {
        std::process::exit(update::cli::run(&arguments[1..]));
    }
    // Squirrel invokes the app before installation is complete. Never start a host then.
    #[cfg(windows)]
    {
        if let Some(event) = arguments.first() {
            if squirrel::handle(event)? { return Ok(()); }
            if event == "--squirrel-firstrun" { arguments.remove(0); }
        }
        squirrel::set_app_user_model_id();
    }
    if let Some(code) = cli_launcher::forward_ssh(&arguments)? {
        std::process::exit(code);
    }
    #[cfg(target_os = "macos")]
    let install_cli = arguments.is_empty();
    let options = Options::parse(arguments)?;
    if options.mode == Mode::Smoke {
        let mut frontend = frontend_composition::FrontendDirectories::plan(&options)?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            frontend.prepare()?;
            smoke::run(&options.run_dir)
        }));
        return finish_frontend(frontend.preserve(), result);
    }
    let show_startup_error = options.mode == Mode::Shell && !options.ui_acceptance;
    let startup = app_startup::prepare(&options).inspect_err(|_| {
        if show_startup_error { desktop_errors::startup(); }
    })?;
    let app_startup::Startup::Graphical(owner) = startup else {
        return Ok(());
    };
    #[cfg(target_os = "macos")]
    if install_cli {
        if let Err(error) = cli_launcher::install_mac() {
            eprintln!("Shellfox CLI integration unavailable: {error:#}");
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
            if show_startup_error { desktop_errors::startup(); }
            return Err(error);
        }
    };
    // Original issuer and panic keeper are OUTSIDE the unwind/error scope.
    // Logging only. Unlike --ui-acceptance this does not change presentation.
    let trace = options.ui_acceptance
        || std::env::var_os("SHELLFOX_UI_INIT_TRACE").is_some_and(|value| value == "1");
    let updater = if options.ui_acceptance || owner.is_none() {
        None
    } else {
        update::Updater::start().map(std::sync::Arc::new).map_err(|error| {
            eprintln!("Updater unavailable: {error:#}");
        }).ok()
    };
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
        run_graphics(options, owner.as_ref(), device_colors, editors, trace, updater.clone())
    }));
    let preserved = frontend.preserve();
    if let Some(owner) = &owner {
        owner.cleanup_until_confirmed();
    }
    if matches!(graphics, Ok(Ok(()))) && let Some(updater) = &updater
        && let Err(error) = updater.install_on_exit()
    {
        eprintln!("Shellfox update did not start: {error:#}");
    }
    finish_frontend(preserved, graphics).inspect_err(|_| {
        if show_startup_error { desktop_errors::startup(); }
    })
}

/// Lets `--version`, `--smoke` and `ssh` print to the terminal that started
/// this GUI binary. Redirected output already has handles and is left alone.
#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(handle: u32) -> *mut std::ffi::c_void;
        fn AttachConsole(process: u32) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: both calls only read or set this process's standard handles.
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
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
    updater: Option<std::sync::Arc<update::Updater>>,
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
            layout_shortcuts::bind_keys(cx);
            #[cfg(target_os = "macos")]
            app_menu::install(cx);
            if let Some(updater) = updater {
                cx.set_global(update_view::Updates(updater));
            }
            trace_ui_init(trace, "prestarted adapter install begin");
            if let Some((config, host)) = retained {
                trace_ui_init(trace, "host wait begin");
                match app_startup::ready(&host, &config, &options) {
                    Ok(app_startup::Ready::Attach(chrome)) => {
                        if let Some(chrome) = chrome { chrome.install(cx); }
                    }
                    Ok(app_startup::Ready::Forwarded) => {
                        cx.quit();
                        return;
                    }
                    Err(error) => {
                        app_startup::record_init_failure(error, cx);
                        cx.quit();
                        return;
                    }
                }
                trace_ui_init(trace, "host wait returned");
                if let Err(error) = app_host::adapter::install_prestarted(cx, config.clone(), &host) {
                    app_startup::record_init_failure(error, cx);
                    cx.quit();
                    return;
                }
                cx.set_global(host_lost::Relaunch(options.relaunch_args.clone()));
                host_lost::watch(config, host, cx);
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
            desktop_windows::bootstrap(options, initial, trace, cx);
        });
    trace_ui_init(trace, "Application run returned");
    let error = errors.0.borrow_mut().take();
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
