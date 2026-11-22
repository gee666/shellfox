//! Compile the actual accessibility module without importing the desktop Root/UI.
// Only the bin uses some items, so partial roots like this one see them as dead.
#![allow(dead_code)]
#[path = "accessibility.rs"]
pub mod accessibility;
