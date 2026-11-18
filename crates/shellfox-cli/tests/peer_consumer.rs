//! Compile the actual native source cohort; default availability is tested in args.rs.
#![cfg(feature = "experimental-shellfox-native")]
#[cfg(feature = "experimental-shellfox-native")]
#[path = "../src/terminal_guard.rs"]
pub mod attach;
#[path = "../src/peer.rs"]
mod peer;
#[path = "../src/peer_args.rs"]
mod peer_args;
#[cfg(feature = "experimental-shellfox-native")]
#[path = "../src/presentation.rs"]
pub mod presentation;
#[test]
fn actual_cli_route_is_type_checked_without_opening_a_host() {
    let _route = peer::run;
}
