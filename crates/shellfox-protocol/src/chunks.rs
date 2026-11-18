use crate::{validation::ensure, ValidationError};

pub const FILE_CHUNK_BYTES: usize = 192 * 1024;
pub const FILE_TRANSFER_MAX_BYTES: u64 = 1024 * 1024 * 1024;
pub const FILE_TRANSFER_DEFAULT_BYTES: u64 = 256 * 1024 * 1024;
pub const TERMINAL_UPLOAD_DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const FILE_TRANSFER_TTL_MS: u64 = 5 * 60_000;
pub fn transfer_size(value: u64) -> Result<(), ValidationError> {
    ensure(value <= FILE_TRANSFER_MAX_BYTES)
}

/// Canonical RFC 4648 base64, including zero unused padding bits.
pub fn chunk_size(data: &str, allow_empty: bool) -> Result<usize, ValidationError> {
    ensure(
        (allow_empty || !data.is_empty())
            && data.len() <= FILE_CHUNK_BYTES / 3 * 4
            && data.len().is_multiple_of(4),
    )?;
    base64_size(data)
}
pub(crate) fn base64_size(data: &str) -> Result<usize, ValidationError> {
    let padding = if data.ends_with("==") {
        2
    } else if data.ends_with('=') {
        1
    } else {
        0
    };
    ensure(data.len().is_multiple_of(4) && data.len() >= padding)?;
    let content = &data.as_bytes()[..data.len() - padding];
    ensure(
        content
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"+/".contains(b)),
    )?;
    if padding == 2 {
        ensure(content.last().is_some_and(|b| b"AQgw".contains(b)))?;
    }
    if padding == 1 {
        ensure(
            content
                .last()
                .is_some_and(|b| b"AEIMQUYcgkosw048".contains(b)),
        )?;
    }
    Ok(data.len() / 4 * 3 - padding)
}
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn encode_chunk(bytes: &[u8]) -> Result<String, ValidationError> {
    ensure(bytes.len() <= FILE_CHUNK_BYTES)?;
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for part in bytes.chunks(3) {
        let n = (u32::from(part[0]) << 16)
            | (u32::from(*part.get(1).unwrap_or(&0)) << 8)
            | u32::from(*part.get(2).unwrap_or(&0));
        result.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        result.push(if part.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if part.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    Ok(result)
}
pub fn decode_chunk(data: &str) -> Result<Vec<u8>, ValidationError> {
    let size = chunk_size(data, true)?;
    let mut bytes = Vec::with_capacity(size);
    for part in data.as_bytes().chunks_exact(4) {
        let value = |b: u8| ALPHABET.iter().position(|v| *v == b).unwrap_or(0) as u32;
        let n = (value(part[0]) << 18)
            | (value(part[1]) << 12)
            | (value(part[2]) << 6)
            | value(part[3]);
        bytes.push((n >> 16) as u8);
        if part[2] != b'=' {
            bytes.push((n >> 8) as u8);
        }
        if part[3] != b'=' {
            bytes.push(n as u8);
        }
    }
    Ok(bytes)
}
