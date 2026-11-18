use crate::{methods::method, validation::*, *};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSession {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub title: Option<String>,
    pub cwd: String,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub request_id: SessionId,
}
impl Validate for CreateSession {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(value) = &self.title {
            title(value)?;
        }
        cwd(&self.cwd)
    }
}
method!(CreateSession, SessionDto, "createSession");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddTab {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub cwd: Option<String>,
}
impl Validate for AddTab {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(value) = &self.title {
            title(value)?;
        }
        if let Some(value) = &self.cwd {
            cwd(value)?;
        }
        if let Some(value) = &self.profile_id {
            profile(value)?;
        }
        Ok(())
    }
}
method!(AddTab, SessionDto, "addTab");
macro_rules! session_method {
    ($ty:ident, $response:ty, $name:literal) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $ty {
            #[serde(deserialize_with = "crate::ids::wire_id")]
            pub session_id: SessionId,
        }
        impl Validate for $ty {
            fn validate(&self) -> Result<(), ValidationError> {
                Ok(())
            }
        }
        method!($ty, $response, $name);
    };
}
session_method!(ActivateSession, SessionDto, "activateSession");
session_method!(UnsettleSession, SessionDto, "unsettleSession");
session_method!(ClearSessionError, SessionDto, "clearSessionError");
session_method!(DeleteSession, Deleted, "deleteSession");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameSession {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    pub title: String,
}
impl Validate for RenameSession {
    fn validate(&self) -> Result<(), ValidationError> {
        title(&self.title)
    }
}
method!(RenameSession, SessionDto, "renameSession");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameTab {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    pub title: String,
}
impl Validate for RenameTab {
    fn validate(&self) -> Result<(), ValidationError> {
        title(&self.title)
    }
}
method!(RenameTab, SessionDto, "renameTab");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReorderTabs {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    #[serde(deserialize_with = "crate::ids::wire_ids")]
    pub tab_ids: Vec<TabId>,
}
impl Validate for ReorderTabs {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(!self.tab_ids.is_empty())?;
        unique(&self.tab_ids, 1000)
    }
}
method!(ReorderTabs, SessionDto, "reorderTabs");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetSessionPinned {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    pub pinned: bool,
}
impl Validate for SetSessionPinned {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
method!(SetSessionPinned, SessionDto, "setSessionPinned");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettleSession {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    pub confirm_active: bool,
}
impl Validate for SettleSession {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
method!(SettleSession, SessionDto, "settleSession");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetSessionEnv {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    pub env: Vec<EnvVar>,
}
impl Validate for SetSessionEnv {
    fn validate(&self) -> Result<(), ValidationError> {
        env(&self.env, false)
    }
}
method!(SetSessionEnv, SessionDto, "setSessionEnv");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTerminalEnv {
    pub env: Vec<EnvVar>,
}
impl Validate for SetTerminalEnv {
    fn validate(&self) -> Result<(), ValidationError> {
        env(&self.env, false)
    }
}
method!(SetTerminalEnv, SettingsDto, "setTerminalEnv");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetVisibleSessions {
    #[serde(deserialize_with = "crate::ids::wire_ids")]
    pub session_ids: Vec<SessionId>,
}
impl Validate for SetVisibleSessions {
    fn validate(&self) -> Result<(), ValidationError> {
        unique(&self.session_ids, 10000)
    }
}
method!(SetVisibleSessions, Configured, "setVisibleSessions");
macro_rules! flag {
    ($ty:ident, $field:ident) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $ty {
            pub $field: bool,
        }
        impl Validate for $ty {
            fn validate(&self) -> Result<(), ValidationError> {
                ensure(self.$field)
            }
        }
    };
}
flag!(Deleted, deleted);
flag!(Configured, configured);
flag!(Taken, taken);
flag!(Written, written);
flag!(Resized, resized);
flag!(Acknowledged, acknowledged);
flag!(Detached, detached);
flag!(Disconnected, disconnected);
