//! Private shared transport/session lifecycle, not an additional wire profile.
mod client;
mod connection;
pub(crate) mod delivery;
mod profiles;
mod state;
mod transport;
pub(crate) use client::{BindingWitness, CommonClient};
pub(crate) use profiles::{ManagerProfile, ObserverProfile, Profile, WireBoundReply, WireReply};
pub use state::{ClientError, DiscoveryOutcome, FallbackPolicy, Status};
