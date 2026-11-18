//! Headless verification of the actual native UI client modules, without GPUI init.
#[path = "host_client.rs"]
pub mod host_client;
#[path = "app_host.rs"]
pub mod app_host;
#[path = "app_launch.rs"]
mod app_launch;
#[path = "window_manager.rs"]
pub mod window_manager;
