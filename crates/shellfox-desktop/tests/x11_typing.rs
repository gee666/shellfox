//! 2,000 characters typed into a focused X11 terminal as fast as xdotool can
//! all reach `cat`, in order. Runs only with SHELLFOX_X11_TYPING=1 under an X
//! server with xdotool, as in linux-check.yml; see support/mod.rs for the
//! other prerequisites.
mod support;

use std::time::Duration;
use support::{
    Desktop, Host, Ui, wait_exit,
    x11::{type_line, wait_for_shell, wait_until, window, xdotool},
};

#[test]
fn two_thousand_fast_keys_reach_the_program_in_order() {
    if std::env::var_os("SHELLFOX_X11_TYPING").is_none_or(|value| value != "1") {
        eprintln!("Skipped: set SHELLFOX_X11_TYPING=1 under an X server with xdotool");
        return;
    }
    let desktop = Desktop::new("desktop-x11-typing-");
    let folder = desktop.data.parent().unwrap().join("work");
    std::fs::create_dir_all(&folder).unwrap();
    let mut ui = Ui(desktop.command(&[]).spawn().unwrap());
    let host = Host(&desktop);
    desktop.wait_registered(Some(&mut ui));
    let status = desktop
        .command(&["add", folder.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success(), "launch forwarding failed: {status}");
    let window = window();
    wait_for_shell(&window, &folder, &mut ui);
    let file = folder.join("typed.txt");
    type_line(&window, "cat > typed.txt");
    // Only keys on the US keymap: xdotool remaps a spare keycode for any
    // other character, which races at zero delay. One line stays below the
    // tty's 4 KiB line limit.
    let sample = "the quick brown fox jumps over the lazy dog 0123456789 ";
    let text: String = sample.chars().cycle().take(2_000).collect();
    std::thread::sleep(Duration::from_secs(1));
    xdotool(&["type", "--delay", "0", &text]);
    xdotool(&["key", "Return", "ctrl+d"]);
    let expected = format!("{text}\n");
    let arrived = wait_until(&mut ui, Duration::from_secs(60), || {
        std::fs::read_to_string(&file).is_ok_and(|typed| typed.len() >= expected.len())
    });
    let typed = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        arrived,
        "{} of {} bytes arrived",
        typed.len(),
        expected.len()
    );
    assert_eq!(typed, expected);
    drop(host);
    wait_exit(
        &mut ui,
        Duration::from_secs(30),
        "The desktop kept running after its host stopped",
    );
}
