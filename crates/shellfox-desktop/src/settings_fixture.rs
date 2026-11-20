//! Inputs for an explicitly launched development acceptance window only.
#[cfg(all(debug_assertions, feature = "test-settings-ui"))]
pub(super) fn value(name: &str) -> Option<String> {
    if !std::env::args_os().any(|arg| arg == "--ui-acceptance") {
        return None;
    }
    std::env::var(format!("SHELLFOX_TEST_SETTINGS_{name}")).ok()
}

#[cfg(not(all(debug_assertions, feature = "test-settings-ui")))]
pub(super) fn value(_: &str) -> Option<String> {
    None
}
