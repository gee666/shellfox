use super::*;
use crate::{validation::ensure, AuthenticatedPeerId, Validate, ValidationError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Discover {}
impl Validate for Discover {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Discovery {
    pub service: ServiceName,
    pub owner_id: Id,
    pub profiles: Vec<String>,
    pub catalog_modes: Vec<String>,
    pub delivery_formats: Vec<String>,
    pub recovery_formats: Vec<String>,
}
impl Discovery {
    pub fn supports_observer(&self) -> bool {
        self.profiles.iter().any(|v| v == PROFILE)
            && self.catalog_modes.iter().any(|v| v == "complete")
            && self
                .delivery_formats
                .iter()
                .any(|v| v == "vt-bytes-base64-v1")
            && self
                .recovery_formats
                .iter()
                .any(|v| v == "formatted-active-buffer-v1")
    }
}
impl Validate for Discovery {
    fn validate(&self) -> Result<(), ValidationError> {
        for values in [
            &self.profiles,
            &self.catalog_modes,
            &self.delivery_formats,
            &self.recovery_formats,
        ] {
            crate::validation::unique(values, 16)?;
            for value in values {
                ensure(
                    !value.is_empty()
                        && value.len() <= 64
                        && value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                )?;
            }
        }
        Ok(())
    }
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
}
impl PairOffer {
    pub fn new(client_id: Id, attempt_id: Id, owner_id: Id) -> Self {
        Self {
            version: Version,
            client_id,
            attempt_id,
            owner_id,
            profile: Profile::Observer,
            catalog_mode: CatalogMode::Complete,
            delivery_format: DeliveryFormat::VtBytes,
            recovery_format: RecoveryFormat::FormattedActiveBuffer,
        }
    }
}
impl Validate for PairOffer {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
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
}
impl PairSelection {
    pub fn validate_offer(&self, offer: &PairOffer) -> Result<(), ValidationError> {
        ensure(
            self.client_id == offer.client_id
                && self.attempt_id == offer.attempt_id
                && self.owner_id == offer.owner_id
                && self.profile == offer.profile
                && self.catalog_mode == offer.catalog_mode
                && self.delivery_format == offer.delivery_format
                && self.recovery_format == offer.recovery_format,
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
        Ok(())
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

/// Server-side association. The peer MUST come from verified transport context,
/// never a body field. A connection UUID alone is not authentication.
#[derive(Clone, Debug)]
pub struct ConnectionBinding {
    peer: AuthenticatedPeerId,
    binding: Binding,
}
impl ConnectionBinding {
    pub fn new(transport_peer: AuthenticatedPeerId, binding: Binding) -> Self {
        Self {
            peer: transport_peer,
            binding,
        }
    }
    pub fn check(
        &self,
        transport_peer: &AuthenticatedPeerId,
        body: &Binding,
    ) -> Result<(), ValidationError> {
        ensure(self.peer == *transport_peer && self.binding == *body)
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
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
pub struct Disconnected {
    pub disconnected: bool,
}
impl Validate for Disconnected {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.disconnected)
    }
}
