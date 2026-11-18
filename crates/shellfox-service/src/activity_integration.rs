//! Owner-thread policy adapters and immutable, sanitized async fact sources.
#[path = "activity_integration_clock.rs"]
mod clock;
#[path = "activity_integration_discovery.rs"]
mod discovery;
#[path = "activity_integration_input.rs"]
mod input;
#[path = "activity_integration_inspector.rs"]
mod inspector;
#[path = "activity_integration_live.rs"]
mod live;
#[path = "activity_integration_owner.rs"]
mod owner;
#[path = "activity_integration_persistence.rs"]
mod persistence;
#[cfg(test)]
#[path = "activity_integration_projection_tests.rs"]
mod projection_tests;
#[path = "activity_integration_projector.rs"]
mod projector;
#[path = "activity_integration_source.rs"]
mod source;
#[path = "activity_integration_startup.rs"]
mod startup;
pub use crate::facts_projection::project_session_facts as project_owned_session_facts;
pub use clock::*;
pub use discovery::{DiscoveredFacts, FactsDiscovery, NativeFactsDiscovery};
pub use input::*;
pub use inspector::*;
pub use live::*;
pub use owner::*;
pub use persistence::*;
pub use projector::NativeSourceFactsProjector;
pub use source::*;
pub use startup::StartupEvidence;

#[cfg(test)]
#[path = "activity_integration_tests.rs"]
mod tests;
