//! Owner-thread policy adapters and immutable, sanitized async fact sources.
mod clock;
mod discovery;
mod input;
mod inspector;
mod live;
mod owner;
mod persistence;
#[cfg(test)]
mod projection_tests;
mod projector;
mod source;
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
mod tests;
