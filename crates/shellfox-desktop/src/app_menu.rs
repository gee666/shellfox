//! The standard macOS menu bar. Windows and Linux have no menu bar.
use gpui::actions;

// Edit items act on the focused terminal or text field. Cmd+C/V/A stay with
// those views' own key handling; the Edit bindings only label the menu.
actions!(shellfox_app, [About, Hide, HideOthers, Quit, Minimize, Copy, Paste, SelectAll]);

#[cfg(target_os = "macos")]
const MENU_LABEL_ONLY: &str = "MenuLabelOnly";

#[cfg(target_os = "macos")]
pub(crate) fn install(cx: &mut gpui::App) {
    use gpui::{KeyBinding, Menu, MenuItem};
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        // No element sets this context, so these never match in a window. GPUI
        // still shows the first binding of an action as the menu's shortcut.
        KeyBinding::new("cmd-c", Copy, Some(MENU_LABEL_ONLY)),
        KeyBinding::new("cmd-v", Paste, Some(MENU_LABEL_ONLY)),
        KeyBinding::new("cmd-a", SelectAll, Some(MENU_LABEL_ONLY)),
    ]);
    cx.on_action(|_: &About, cx| about(cx));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &Quit, cx| quit(cx));
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.set_menus(vec![
        Menu {
            name: "Shellfox".into(),
            items: vec![
                MenuItem::action("About Shellfox", About),
                MenuItem::separator(),
                MenuItem::action("Hide Shellfox", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::separator(),
                MenuItem::action("Quit Shellfox", Quit),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Copy", Copy),
                MenuItem::action("Paste", Paste),
                MenuItem::action("Select All", SelectAll),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![MenuItem::action("Minimize", Minimize)],
        },
    ]);
}

#[cfg(target_os = "macos")]
fn about(cx: &mut gpui::App) {
    let Some(window) = cx.active_window() else {
        return;
    };
    let version = concat!("Version ", env!("SHELLFOX_VERSION"));
    let _ = window.update(cx, |_, window, cx| {
        let _ = window.prompt(gpui::PromptLevel::Info, "Shellfox", Some(version), &["OK"], cx);
    });
}

/// Each window goes through the same confirmation as closing it.
#[cfg(target_os = "macos")]
pub(crate) fn quit(cx: &mut gpui::App) {
    for handle in cx.windows() {
        let Some(root) = handle.downcast::<crate::root::Root>() else {
            continue;
        };
        let _ = root.update(cx, |root, window, cx| {
            if root.close(window, cx) {
                window.remove_window();
            }
        });
    }
}
