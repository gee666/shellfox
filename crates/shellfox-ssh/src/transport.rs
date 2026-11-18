//! Connection boundary for a future retained system-SSH or reviewed Rust adapter.
//! This module does not spawn processes, allocate PTYs, resolve hosts or connect sockets.
use crate::{
    pty::{remote_environment, RemotePty},
    Error, HostKeyInfo, KnownHosts, Result, StoredProfile, TrustDecision,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionTarget {
    pub host: String,
    pub port: u16,
    pub address_family: u8,
}
/// A handshake may exchange public information only. Authentication must wait for trust.
pub trait TransportFactory {
    fn handshake(&self, target: &ConnectionTarget) -> Result<Box<dyn PendingTransport>>;
}
pub trait PendingTransport {
    fn host_key(&self) -> &[u8];
    /// Return only after authentication and a real remote PTY/shell have succeeded.
    /// The adapter must stop its owned connection on error or Drop.
    fn authenticate_and_open(
        self: Box<Self>,
        profile: &StoredProfile,
        pty: &RemotePty,
        environment: &BTreeMap<String, String>,
    ) -> Result<Box<dyn SshSession>>;
}
pub trait SshSession {
    fn write(&mut self, bytes: &[u8]) -> Result<usize>;
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize>;
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;
    /// Success requires confirmed remote exit, not simply EOF or a dropped socket.
    fn wait(&mut self) -> Result<i32>;
    fn close(&mut self) -> Result<()>;
}
pub struct UnavailableTransport;
impl TransportFactory for UnavailableTransport {
    fn handshake(&self, _: &ConnectionTarget) -> Result<Box<dyn PendingTransport>> {
        Err(Error::AdapterUnavailable)
    }
}
/// Credentials never reach the adapter's handshake or an untrusted peer.
/// Remote-only mobile clients must call host RPCs with profile IDs, not this function.
pub fn connect<'a>(
    factory: &dyn TransportFactory,
    known_hosts: &KnownHosts,
    profile: &StoredProfile,
    pty: RemotePty,
    env: impl IntoIterator<Item = (&'a str, &'a str)>,
    confirm: impl FnOnce(&str, u16, &HostKeyInfo) -> Result<TrustDecision>,
) -> Result<Box<dyn SshSession>> {
    let profile = profile.clone().normalize()?;
    let pty = RemotePty::new(pty.cols, pty.rows)?;
    if profile
        .connection_options
        .as_ref()
        .is_some_and(|o| o.unsupported_proxy.is_some())
    {
        return Err(Error::UnsupportedProxy);
    }
    let target = ConnectionTarget {
        host: profile.host.clone(),
        port: profile.port,
        address_family: profile
            .connection_options
            .as_ref()
            .and_then(|o| o.address_family)
            .unwrap_or(0),
    };
    let pending = factory.handshake(&target)?;
    known_hosts.verify(&target.host, target.port, pending.host_key(), confirm)?;
    pending.authenticate_and_open(&profile, &pty, &remote_environment(env))
}
