use serde::{de::DeserializeOwned, Serialize};
use shellfox_protocol::{manager_v2 as m, observer_v2 as o, ErrorCode, Validate, ValidationError};
pub(crate) trait WireReply<T, E> {
    fn result(self) -> Result<T, E>;
}
pub(crate) trait WireBoundReply<T, B, E> {
    fn parts(self) -> (B, Result<T, E>);
}
macro_rules! wire {
    ($module:ident) => {
        impl<T> WireReply<T, $module::Error> for $module::Reply<T> {
            fn result(self) -> Result<T, $module::Error> {
                match self {
                    Self::Success(v) => Ok(v),
                    Self::Failure(e) => Err(e),
                }
            }
        }
        impl<T> WireBoundReply<T, $module::Binding, $module::Error> for $module::BoundReply<T> {
            fn parts(self) -> ($module::Binding, Result<T, $module::Error>) {
                (self.binding, self.outcome.result())
            }
        }
    };
}
wire!(o);
wire!(m);
pub(crate) trait Profile {
    type Binding: Clone + PartialEq + Serialize + DeserializeOwned + Validate;
    type Offer: Serialize;
    type Selection: DeserializeOwned + Validate;
    type Error: Clone + std::fmt::Debug + DeserializeOwned + Validate;
    type Disconnect: Serialize;
    type Poll: DeserializeOwned + Validate;
    type Reply<T>: DeserializeOwned + Validate + WireReply<T, Self::Error>
    where
        T: DeserializeOwned + Validate;
    type Bound<T>: DeserializeOwned + Validate + WireBoundReply<T, Self::Binding, Self::Error>
    where
        T: DeserializeOwned + Validate;
    fn supports(discovery: &o::Discovery) -> bool;
    fn offer(client: o::Id, attempt: o::Id, owner: o::Id) -> Self::Offer;
    fn selection(
        value: &Self::Selection,
        offer: &Self::Offer,
    ) -> Result<Self::Binding, ValidationError>;
    fn code(error: &Self::Error) -> ErrorCode;
    fn incompatible(error: &Self::Error) -> bool;
    fn disconnect(
        binding: Option<Self::Binding>,
        client: o::Id,
        attempt: Option<o::Id>,
    ) -> Option<Self::Disconnect>;
}
pub(crate) struct ObserverProfile;
pub(crate) struct ManagerProfile;
macro_rules! profile {
    ($name:ident,$module:ident,$supports:expr) => {
        impl Profile for $name {
            type Binding = $module::Binding;
            type Offer = $module::PairOffer;
            type Selection = $module::PairSelection;
            type Error = $module::Error;
            type Disconnect = $module::Disconnect;
            type Poll = $module::PollResponse;
            type Reply<T>
                = $module::Reply<T>
            where
                T: DeserializeOwned + Validate;
            type Bound<T>
                = $module::BoundReply<T>
            where
                T: DeserializeOwned + Validate;
            fn supports(value: &o::Discovery) -> bool {
                ($supports)(value)
            }
            fn offer(client: o::Id, attempt: o::Id, owner: o::Id) -> Self::Offer {
                $module::PairOffer::new(client, attempt, owner)
            }
            fn selection(
                value: &Self::Selection,
                offer: &Self::Offer,
            ) -> Result<Self::Binding, ValidationError> {
                value.validate_offer(offer)?;
                Ok(value.binding())
            }
            fn code(error: &Self::Error) -> ErrorCode {
                error.code
            }
            fn incompatible(error: &Self::Error) -> bool {
                error.code == ErrorCode::Unsupported
                    && matches!(error.detail, $module::ErrorDetail::ProfileIncompatible)
            }
            fn disconnect(
                binding: Option<Self::Binding>,
                client: o::Id,
                attempt: Option<o::Id>,
            ) -> Option<Self::Disconnect> {
                if let Some(binding) = binding {
                    Some($module::Disconnect::Connection {
                        version: o::Version,
                        binding,
                    })
                } else {
                    attempt.map(|attempt_id| $module::Disconnect::Attempt {
                        version: o::Version,
                        client_id: client,
                        attempt_id,
                    })
                }
            }
        }
    };
}
profile!(ObserverProfile, o, |value: &o::Discovery| value
    .supports_observer());
profile!(ManagerProfile, m, m::supports_manager);
