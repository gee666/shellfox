use super::{
    discovery::WslProfileId,
    protocol::{token, WslError},
    OsWsl,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct WslSecret([u8; 32]);
impl WslSecret {
    /// Supply 32 bytes from the platform CSPRNG. Never persist in preferences/logs.
    pub fn new(bytes: [u8; 32]) -> Result<Self, WslError> {
        if bytes == [0; 32] {
            return Err(WslError::InvalidRequest);
        }
        Ok(Self(bytes))
    }
    pub(crate) fn encoded(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}
impl std::fmt::Debug for WslSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WslSecret([redacted])")
    }
}
/// Created only by an explicit user-approved WSL operation. Discovery never uses it.
#[derive(Clone, Copy, Debug)]
pub struct WslConsent(());
impl WslConsent {
    pub fn explicit_user_approval() -> Self {
        Self(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuestSettings {
    pub(crate) python: String,
    pub(crate) bundle_directory: String,
}
impl GuestSettings {
    /// Absolute trusted guest Python from Settings and provisioned immutable scripts.
    /// Python always starts with -I -S; bootstrap verifies every bundled source hash.
    pub fn new(python: String, bundle_directory: String) -> Result<Self, WslError> {
        guest_path(&python)?;
        guest_path(&bundle_directory)?;
        Ok(Self {
            python,
            bundle_directory,
        })
    }
    pub(crate) fn artifacts(&self) -> Value {
        Value::Array(super::SOURCES.iter().map(|(name,source)| {
            let filename=match *name {"shellfox_wsl_wire"=>"wire.py","shellfox_wsl_tree"=>"tree.py","shellfox_wsl_child"=>"child.py",_=>"guardian.py"};
            json!({"name":name,"path":format!("{}/{filename}",self.bundle_directory),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))})
        }).collect())
    }
}
#[derive(Clone, Debug)]
pub struct GuestCommand {
    pub executable: String,
    pub args: Vec<String>,
}
impl GuestCommand {
    pub fn bash() -> Self {
        Self {
            executable: "/bin/bash".into(),
            args: vec!["--noprofile".into(), "--norc".into(), "-i".into()],
        }
    }
    pub(crate) fn validate(&self) -> Result<(), WslError> {
        guest_path(&self.executable)?;
        if self.args.len() > 64 || self.args.iter().any(|s| s.len() > 8192 || s.contains('\0')) {
            return Err(WslError::InvalidRequest);
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct WslConfig {
    pub executable: OsWsl,
    pub distro: WslProfileId,
    pub settings: GuestSettings,
    pub state_directory: String,
    pub cwd: String,
    pub command: GuestCommand,
    pub cols: u16,
    pub rows: u16,
    pub marker: String,
    pub secret: WslSecret,
    pub consent: WslConsent,
}
impl WslConfig {
    pub(crate) fn validate(&self) -> Result<(), WslError> {
        guest_path(&self.state_directory)?;
        guest_path(&self.cwd)?;
        self.command.validate()?;
        if self.state_directory.len() > 80
            || !token(&self.marker)
            || self.marker.eq_ignore_ascii_case(&self.secret.encoded())
        {
            return Err(WslError::InvalidRequest);
        }
        dimensions(self.cols, self.rows)?;
        if serde_json::to_vec(&self.payload())
            .map_err(|_| WslError::InvalidRequest)?
            .len()
            > super::protocol::FRAME_BYTES
        {
            return Err(WslError::InvalidRequest);
        }
        Ok(())
    }
    pub(crate) fn payload(&self) -> Value {
        json!({"mode":"launch","secret":self.secret.encoded(),"marker":self.marker,"state":self.state_directory,
            "cwd":self.cwd,"program":self.command.executable,"args":self.command.args,"cols":self.cols,"rows":self.rows})
    }
}
/// Cannot be constructed from cached profiles, inherited markers or caller claims.
#[derive(Debug)]
pub struct NativeWslCapability {
    pub(crate) distro: WslProfileId,
    pub(crate) settings: GuestSettings,
    pub(crate) boot: String,
    pub(crate) executable: OsWsl,
}
pub(crate) fn guest_path(path: &str) -> Result<(), WslError> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || path.split('/').any(|s| s == "." || s == "..")
        || path.len() > 1 && path.ends_with('/')
    {
        return Err(WslError::InvalidRequest);
    }
    Ok(())
}
pub(crate) fn dimensions(cols: u16, rows: u16) -> Result<(), WslError> {
    if cols == 0
        || rows == 0
        || cols > 32767
        || rows > 32767
        || u32::from(cols) * u32::from(rows) > 1024 * 1024
    {
        return Err(WslError::InvalidRequest);
    }
    Ok(())
}
