use super::*;
use crate::{validation::ensure, AuthenticatedPeerId, Validate, ValidationError};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Profile {
    #[default]
    #[serde(rename = "manager-v2")]
    Manager,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum InputFormat {
    #[default]
    #[serde(rename = "manager-input-v1")]
    Typed,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum DevicePolicy {
    #[default]
    #[serde(rename = "control-full-account-v1")]
    ControlFullAccount,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum DevicePermission {
    #[default]
    #[serde(rename = "write")]
    Write,
}
pub fn supports_manager(value: &Discovery) -> bool {
    value.profiles.iter().any(|v| v == PROFILE)
        && value.catalog_modes.iter().any(|v| v == "complete")
        && value
            .delivery_formats
            .iter()
            .any(|v| v == "vt-bytes-base64-v1")
        && value
            .recovery_formats
            .iter()
            .any(|v| v == "formatted-active-buffer-v1" || v == "native0-raw-v1")
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairOffer {
    pub version: Version,
    pub client_id: Id,
    pub attempt_id: Id,
    pub owner_id: Id,
    pub profile: Profile,
    pub catalog_mode: CatalogMode,
    pub delivery_format: DeliveryFormat,
    pub recovery_format: RecoveryFormat,
    pub input_format: InputFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native0_receiver: Option<Native0Codec>,
}
impl PairOffer {
    pub fn new(client_id: Id, attempt_id: Id, owner_id: Id) -> Self {
        Self {
            version: Version,
            client_id,
            attempt_id,
            owner_id,
            profile: Profile::Manager,
            catalog_mode: CatalogMode::Complete,
            delivery_format: DeliveryFormat::VtBytes,
            recovery_format: RecoveryFormat::FormattedActiveBuffer,
            input_format: InputFormat::Typed,
            native0_receiver: None,
        }
    }
    pub fn native0(client_id: Id, attempt_id: Id, owner_id: Id, receiver: Native0Codec) -> Self {
        Self {
            recovery_format: RecoveryFormat::Native0RawV1,
            native0_receiver: Some(receiver),
            ..Self::new(client_id, attempt_id, owner_id)
        }
    }
}
impl Validate for PairOffer {
    fn validate(&self) -> Result<(), ValidationError> {
        match (self.recovery_format, &self.native0_receiver) {
            (RecoveryFormat::FormattedActiveBuffer, None) => Ok(()),
            (RecoveryFormat::Native0RawV1, Some(receiver)) => receiver.validate(),
            _ => Err(ValidationError),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairSelection {
    pub client_id: Id,
    pub attempt_id: Id,
    pub owner_id: Id,
    pub connection_id: Id,
    pub profile: Profile,
    pub catalog_mode: CatalogMode,
    pub delivery_format: DeliveryFormat,
    pub recovery_format: RecoveryFormat,
    pub input_format: InputFormat,
    pub device_policy: DevicePolicy,
    pub device_permission: DevicePermission,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native0_sender: Option<Native0Codec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native0_receiver: Option<Native0Codec>,
}
impl PairSelection {
    pub fn validate_offer(&self, offer: &PairOffer) -> Result<(), ValidationError> {
        self.validate()?;
        offer.validate()?;
        ensure(
            self.client_id == offer.client_id
                && self.attempt_id == offer.attempt_id
                && self.owner_id == offer.owner_id
                && self.profile == offer.profile
                && self.catalog_mode == offer.catalog_mode
                && self.delivery_format == offer.delivery_format
                && self.recovery_format == offer.recovery_format
                && self.input_format == offer.input_format
                && self.native0_receiver == offer.native0_receiver,
        )
    }
    pub fn binding(&self) -> Binding {
        Binding {
            client_id: self.client_id,
            owner_id: self.owner_id,
            connection_id: self.connection_id,
            profile: self.profile,
        }
    }
}
impl Validate for PairSelection {
    fn validate(&self) -> Result<(), ValidationError> {
        match (
            self.recovery_format,
            &self.native0_sender,
            &self.native0_receiver,
        ) {
            (RecoveryFormat::FormattedActiveBuffer, None, None) => Ok(()),
            (RecoveryFormat::Native0RawV1, Some(sender), Some(receiver)) => {
                sender.validate()?;
                receiver.validate()
            }
            _ => Err(ValidationError),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub client_id: Id,
    pub owner_id: Id,
    pub connection_id: Id,
    pub profile: Profile,
}
impl Validate for Binding {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
/// Server association only. The peer must originate in authenticated transport context.
pub struct ConnectionBinding {
    peer: AuthenticatedPeerId,
    binding: Binding,
}
impl ConnectionBinding {
    pub fn new(peer: AuthenticatedPeerId, binding: Binding) -> Self {
        Self { peer, binding }
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn check(
        &self,
        peer: &AuthenticatedPeerId,
        binding: &Binding,
    ) -> Result<(), ValidationError> {
        ensure(self.peer == *peer && self.binding == *binding)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Disconnect {
    #[serde(rename_all = "camelCase")]
    Attempt {
        version: Version,
        client_id: Id,
        attempt_id: Id,
    },
    Connection {
        version: Version,
        binding: Binding,
    },
}
impl Validate for Disconnect {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundPoll {
    pub version: Version,
    pub binding: Binding,
}
impl Validate for BoundPoll {
    fn validate(&self) -> Result<(), ValidationError> {
        self.binding.validate()
    }
}
