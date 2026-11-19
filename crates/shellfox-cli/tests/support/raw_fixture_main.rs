#[cfg(windows)]
include!("../../../shellfox-terminal/tests/support/raw_process_fixture.rs");

#[cfg(not(windows))]
fn main() {}
