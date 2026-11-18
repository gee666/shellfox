//! Same strict observer envelopes, implemented by the private shared v2 codec.
pub type Reply<T> = crate::v2_replies::Reply<T, super::Error>;
pub type BoundReply<T> = crate::v2_replies::BoundReply<T, super::Binding, super::Error>;
