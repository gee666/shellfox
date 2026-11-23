#[cfg(any(windows, target_os = "macos"))]
include!("../../../shellfox-terminal/tests/support/raw_process_fixture.rs");

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {}
