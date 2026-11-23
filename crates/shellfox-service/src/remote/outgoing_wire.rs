//! Closed PRIVATE Host IPC DTOs; the peer Manager protocol remains unchanged.
use super::outgoing::{self, PeerResponse};
use serde::{Deserialize, Serialize};
use shellfox_protocol::Endpoint;
use shellfox_protocol::{AuthenticatedPeerId, RequestLimits, TransportError};
use std::time::{Duration, Instant};
pub(crate) const METHOD: &str = "private.peer.v1";
pub(crate) const MAX_REQUEST: usize = 2 * shellfox_protocol::MAX_REQUEST_BYTES;
pub(crate) const MAX_RESPONSE: usize = 2 * shellfox_protocol::MAX_RESPONSE_BYTES;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub peer: AuthenticatedPeerId,
    pub endpoint: Endpoint,
    chunks: Vec<String>,
    timeout_millis: u32,
    max_response_bytes: usize,
}
impl Request {
    pub fn new(
        peer: &AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: &RequestLimits,
    ) -> Result<Self, TransportError> {
        let limits = outgoing::validate(&body, limits.clone())?;
        let remaining = limits
            .deadline
            .saturating_duration_since(Instant::now())
            .as_millis();
        let timeout_millis =
            u32::try_from(remaining.min(30_000)).map_err(|_| TransportError::Deadline)?;
        if timeout_millis == 0 {
            return Err(TransportError::Deadline);
        }
        Ok(Self {
            peer: peer.clone(),
            endpoint,
            chunks: encode(&body)?,
            timeout_millis,
            max_response_bytes: limits.max_response_bytes,
        })
    }
    pub fn into_parts(
        self,
        cancel: shellfox_protocol::CancellationToken,
    ) -> Result<(AuthenticatedPeerId, Endpoint, Vec<u8>, RequestLimits), TransportError> {
        self.check()?;
        let body = decode(&self.chunks, shellfox_protocol::MAX_REQUEST_BYTES)?;
        let limits = RequestLimits {
            deadline: Instant::now() + Duration::from_millis(u64::from(self.timeout_millis)),
            cancel,
            max_request_bytes: shellfox_protocol::MAX_REQUEST_BYTES,
            max_response_bytes: self.max_response_bytes,
        };
        let limits = outgoing::validate(&body, limits)?;
        Ok((self.peer, self.endpoint, body, limits))
    }
    fn check(&self) -> Result<(), TransportError> {
        if !(1..=30_000).contains(&self.timeout_millis)
            || self.max_response_bytes > shellfox_protocol::MAX_RESPONSE_BYTES
        {
            return Err(TransportError::InvalidRequest);
        }
        let _ = decode(&self.chunks, shellfox_protocol::MAX_REQUEST_BYTES)?;
        Ok(())
    }
}
impl shellfox_protocol::Validate for Request {
    fn validate(&self) -> Result<(), shellfox_protocol::ValidationError> {
        self.check().map_err(|_| shellfox_protocol::ValidationError)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Response {
    status: u16,
    chunks: Vec<String>,
}
impl Response {
    pub fn from_http(response: PeerResponse, limit: usize) -> Result<Self, TransportError> {
        check_response(&response, limit)?;
        Ok(Self {
            status: response.status,
            chunks: encode(&response.body)?,
        })
    }
    pub fn into_http(self, limit: usize) -> Result<PeerResponse, TransportError> {
        let response = PeerResponse {
            status: self.status,
            body: decode(&self.chunks, limit)?,
        };
        check_response(&response, limit)?;
        Ok(response)
    }
}
pub(crate) fn check_response(response: &PeerResponse, limit: usize) -> Result<(), TransportError> {
    if !(200..=599).contains(&response.status) {
        return Err(TransportError::InvalidResponse);
    }
    if response.body.len() > limit.min(shellfox_protocol::MAX_RESPONSE_BYTES) {
        return Err(TransportError::TooLarge);
    }
    if (200..300).contains(&response.status) {
        serde_json::from_slice::<serde_json::Value>(&response.body)
            .map_err(|_| TransportError::InvalidResponse)?;
    }
    Ok(())
}
fn encode(bytes: &[u8]) -> Result<Vec<String>, TransportError> {
    bytes
        .chunks(shellfox_protocol::FILE_CHUNK_BYTES)
        .map(|part| shellfox_protocol::encode_chunk(part).map_err(|_| TransportError::TooLarge))
        .collect()
}
fn decode(chunks: &[String], limit: usize) -> Result<Vec<u8>, TransportError> {
    if chunks.len() > limit.div_ceil(shellfox_protocol::FILE_CHUNK_BYTES) {
        return Err(TransportError::TooLarge);
    }
    let mut bytes = Vec::new();
    for chunk in chunks {
        if chunk.len() > shellfox_protocol::FILE_CHUNK_BYTES.div_ceil(3) * 4 {
            return Err(TransportError::TooLarge);
        }
        let decoded =
            shellfox_protocol::decode_chunk(chunk).map_err(|_| TransportError::InvalidResponse)?;
        if decoded.len() > limit.saturating_sub(bytes.len()) {
            return Err(TransportError::TooLarge);
        }
        bytes.extend_from_slice(&decoded);
    }
    Ok(bytes)
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Failure {
    Cancelled,
    Deadline,
    Unavailable,
    TooLarge,
    Authentication,
    InvalidRequest,
    InvalidResponse,
}
impl From<TransportError> for Failure {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::Cancelled => Self::Cancelled,
            TransportError::Deadline => Self::Deadline,
            TransportError::Unavailable => Self::Unavailable,
            TransportError::TooLarge => Self::TooLarge,
            TransportError::Authentication => Self::Authentication,
            TransportError::InvalidRequest => Self::InvalidRequest,
            TransportError::InvalidResponse | TransportError::Rejected { .. } => {
                Self::InvalidResponse
            }
        }
    }
}
impl From<Failure> for TransportError {
    fn from(error: Failure) -> Self {
        match error {
            Failure::Cancelled => Self::Cancelled,
            Failure::Deadline => Self::Deadline,
            Failure::Unavailable => Self::Unavailable,
            Failure::TooLarge => Self::TooLarge,
            Failure::Authentication => Self::Authentication,
            Failure::InvalidRequest => Self::InvalidRequest,
            Failure::InvalidResponse => Self::InvalidResponse,
        }
    }
}
pub(crate) type Reply = Result<Response, Failure>;
