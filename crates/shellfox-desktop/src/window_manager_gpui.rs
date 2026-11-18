//! Native window adapter only. No layout/style or host policy lives here.
use super::state::{MIN_HEIGHT, MIN_WIDTH, Monitor, RestoredBounds};
use anyhow::{Result, ensure};
use gpui::{
    App, Bounds, KeyBinding, Menu, MenuItem, Window, WindowBounds, WindowOptions, actions, point,
    px, size,
};
use std::rc::Rc;

actions!(
    shellfox_window,
    [
        NewWindow,
        OpenSessionNewWindow,
        CloseWindow,
        QuitApplication,
        NextWindow,
        PreviousWindow
    ]
);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowAction {
    NewWindow,
    OpenSessionNewWindow,
    CloseWindow,
    Quit,
    NextWindow,
    PreviousWindow,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutPlatform {
    Mac,
    WindowsLinux,
}
pub fn key_bindings(platform: ShortcutPlatform) -> Vec<KeyBinding> {
    let modifier = if platform == ShortcutPlatform::Mac {
        "cmd"
    } else {
        "ctrl"
    };
    vec![
        KeyBinding::new(&format!("{modifier}-shift-n"), NewWindow, Some("Workspace")),
        KeyBinding::new(
            &format!("{modifier}-shift-w"),
            CloseWindow,
            Some("Workspace"),
        ),
        KeyBinding::new(&format!("{modifier}-q"), QuitApplication, Some("Workspace")),
        KeyBinding::new(
            &format!("{modifier}-alt-pagedown"),
            NextWindow,
            Some("Workspace"),
        ),
        KeyBinding::new(
            &format!("{modifier}-alt-pageup"),
            PreviousWindow,
            Some("Workspace"),
        ),
    ]
}
/// Register once at application scope. Root consumes OpenSessionNewWindow using
/// its *local* selected session. Quit/Close always enter AppHost's close policy.
pub fn install_actions(cx: &mut App, dispatch: impl Fn(WindowAction, &mut App) + 'static) {
    let dispatch = Rc::new(dispatch);
    macro_rules! handler {
        ($action:ty, $command:expr) => {{
            let dispatch = dispatch.clone();
            cx.on_action::<$action>(move |_, cx| dispatch($command, cx));
        }};
    }
    handler!(NewWindow, WindowAction::NewWindow);
    handler!(OpenSessionNewWindow, WindowAction::OpenSessionNewWindow);
    handler!(CloseWindow, WindowAction::CloseWindow);
    handler!(QuitApplication, WindowAction::Quit);
    handler!(NextWindow, WindowAction::NextWindow);
    handler!(PreviousWindow, WindowAction::PreviousWindow);
}
/// Merge with existing native menus; do not replace application/settings menus.
pub fn window_menu() -> Menu {
    Menu {
        name: "Window".into(),
        items: vec![
            MenuItem::action("New window", NewWindow),
            MenuItem::action("Open session in new window", OpenSessionNewWindow),
            MenuItem::action("Close window", CloseWindow),
            MenuItem::separator(),
            MenuItem::action("Next window", NextWindow),
            MenuItem::action("Previous window", PreviousWindow),
        ],
    }
}
pub fn window_options(
    mut options: WindowOptions,
    restored: RestoredBounds,
    primary: &Monitor,
) -> Result<WindowOptions> {
    ensure!(primary.valid(), "A valid primary monitor is required.");
    let area = primary.work_area;
    let (x, y) = restored.origin.unwrap_or((
        area.x + ((area.width - restored.width) / 2).max(0),
        area.y + ((area.height - restored.height) / 2).max(0),
    ));
    let bounds = Bounds::new(
        point(px(x as f32), px(y as f32)),
        size(px(restored.width as f32), px(restored.height as f32)),
    );
    options.window_bounds = Some(if restored.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    });
    options.window_min_size = Some(size(px(MIN_WIDTH as f32), px(MIN_HEIGHT as f32)));
    // AccessKit's native adapter is per OS window. Native macOS tab grouping
    // breaks that mapping; application tabs remain ordinary Shellfox tabs.
    options.tabbing_identifier = None;
    Ok(options)
}
pub fn configure_opened_window(window: &Window) {
    window.set_tabbing_identifier(None);
}
