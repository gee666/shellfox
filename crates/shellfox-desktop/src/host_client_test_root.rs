//! Headless verification of the actual native UI client modules, without GPUI init.
// Only the bin uses some items, so partial roots like this one see them as dead.
#![allow(dead_code)]
#[path = "host_client.rs"]
pub mod host_client;
#[path = "host_process.rs"]
mod host_process;
#[path = "app_host.rs"]
pub mod app_host;
#[path = "app_launch.rs"]
mod app_launch;
#[path = "window_manager.rs"]
pub mod window_manager;
