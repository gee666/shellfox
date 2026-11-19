use shellfox_ssh::pty::{remote_environment, TTY_MODES};
use std::collections::BTreeMap;

#[test]
fn pty_modes_and_locale_match_the_electron_contract() {
    let modes = TTY_MODES.iter().copied().collect::<BTreeMap<_, _>>();
    assert_eq!(modes.get(&42), Some(&1));
    assert_eq!(modes.get(&3), Some(&127));
    let env = remote_environment([
        ("LANG", "C.UTF-8"),
        ("LC_CTYPE", "en_US.UTF-8"),
        ("LC_ALL", "bad\nvalue"),
        ("NO_COLOR", "1"),
        ("TOKEN", "secret"),
    ]);
    assert_eq!(env.len(), 4);
    assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
    assert!(!env.contains_key("TOKEN"));
    assert!(!env.contains_key("NO_COLOR"));
}
