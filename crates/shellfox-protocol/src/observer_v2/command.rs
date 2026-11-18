use super::*;
use crate::{validation::ensure, Validate, ValidationError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

macro_rules! commands {
    ($($name:ident=$wire:literal),+$(,)?)=>{
        #[derive(Clone,Debug,Serialize,Deserialize)]
        #[serde(tag="method",content="payload",deny_unknown_fields)]
        pub enum Command{$(#[serde(rename=$wire)]$name($name)),+}
        pub const METHODS:&[&str]=&[$($wire),+];
        impl Command {
            pub fn method_name(&self)->&'static str{match self{$(Self::$name(_)=>$wire),+}}
            pub fn is_profile_allowed(&self)->bool{match self{$(Self::$name(_)=><$name as Request>::ALLOWED),+}}
            pub fn authorize_profile(&self)->Result<(),Error>{if self.is_profile_allowed(){Ok(())}else{Err(Error::profile_denied())}}
        }
        impl Validate for Command{fn validate(&self)->Result<(),ValidationError>{match self{$(Self::$name(v)=>v.validate_wire()),+}}}
        pub fn decode_request(bytes:&[u8])->Result<DecodedRequest,ValidationError>{
            ensure(bytes.len()<=crate::MAX_REQUEST_BYTES)?;
            let header:Header=serde_json::from_slice(bytes).map_err(|_|ValidationError)?;
            let binding=header.binding;
            let command=match header.method.as_str(){
                $($wire=>{
                    let envelope:Envelope<$name>=serde_json::from_slice(bytes).map_err(|_|ValidationError)?;
                    envelope.validate()?;Command::$name(envelope.payload)
                }),+,
                _=>return Err(ValidationError),
            };
            Ok(DecodedRequest{binding,command})
        }
    };
}
commands! {
    GetSnapshot="getSnapshot",
    GetTerminalProfiles="getTerminalProfiles",
    GetHomeDirectory="getHomeDirectory",
    CreateSession="createSession",
    AddTab="addTab",
    ActivateSession="activateSession",
    RenameSession="renameSession",
    RenameTab="renameTab",
    ReorderTabs="reorderTabs",
    SetSessionPinned="setSessionPinned",
    SettleSession="settleSession",
    UnsettleSession="unsettleSession",
    DeleteSession="deleteSession",
    ClearSessionError="clearSessionError",
    SetSessionEnv="setSessionEnv",
    SetTerminalEnv="setTerminalEnv",
    SetVisibleSessions="setVisibleSessions",
    AttachTerminal="attachTerminal",
    TakeTerminalControl="takeTerminalControl",
    WriteTerminal="writeTerminal",
    ResizeTerminal="resizeTerminal",
    AcknowledgeTerminal="acknowledgeTerminal",
    DetachTerminal="detachTerminal",
    CloseTab="closeTab",
    ListFiles="listFiles",
    ReadFile="readFile",
    WriteFile="writeFile",
    BeginFileRead="beginFileRead",
    ReadFileChunk="readFileChunk",
    BeginFileWrite="beginFileWrite",
    WriteFileChunk="writeFileChunk",
    FinishFileWrite="finishFileWrite",
    CancelFileTransfer="cancelFileTransfer",
    WriteImageFile="writeImageFile",
    ReadFileAsset="readFileAsset",
    GetSessionStats="getSessionStats",
    ResolveDirectory="resolveDirectory",
    CreateDirectory="createDirectory",
    CompleteDirectory="completeDirectory",
    BeginTerminalUpload="beginTerminalUpload",
    WriteTerminalUpload="writeTerminalUpload",
    FinishTerminalUpload="finishTerminalUpload",
    CancelTerminalUpload="cancelTerminalUpload",
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "R: DeserializeOwned"))]
pub struct Envelope<R: Request> {
    pub version: Version,
    pub binding: Binding,
    pub method: String,
    pub payload: R,
}
impl<R: Request> Envelope<R> {
    pub fn new(binding: Binding, payload: R) -> Self {
        Self {
            version: Version,
            binding,
            method: R::METHOD.into(),
            payload,
        }
    }
}
impl<R: Request> Validate for Envelope<R> {
    fn validate(&self) -> Result<(), ValidationError> {
        self.binding.validate()?;
        ensure(self.method == R::METHOD)?;
        self.payload.validate_wire()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    #[serde(rename = "version")]
    _version: Version,
    binding: Binding,
    method: String,
    #[serde(rename = "payload")]
    _payload: serde::de::IgnoredAny,
}
pub struct DecodedRequest {
    pub binding: Binding,
    pub command: Command,
}
impl DecodedRequest {
    pub fn check_connection(
        &self,
        connection: &ConnectionBinding,
        transport_peer: &crate::AuthenticatedPeerId,
    ) -> Result<(), ValidationError> {
        connection.check(transport_peer, &self.binding)
    }
}
