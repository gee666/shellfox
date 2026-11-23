use crate::{
    limits::validate_response, AuthenticatedPeerId, Endpoint, HelperTransport, HttpResponse,
    OutboundTransport, RequestLimits, TransportError, TransportFuture,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

impl OutboundTransport for HelperTransport {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let response = self
                .request_http(peer, endpoint, body, limits, true)
                .await?;
            legacy_response(response)
        })
    }
}
fn legacy_response(response: HttpResponse) -> Result<Vec<u8>, TransportError> {
    if !(200..300).contains(&response.status) {
        return Err(TransportError::Rejected {
            status: response.status,
        });
    }
    Ok(response.body)
}

// One fresh connection, one POST. There is no retry path, redirect handler,
// environment proxy, cookies, DNS lookup or externally supplied header/URL.
pub(super) async fn post_with_status(
    address: std::net::SocketAddr,
    secret: &str,
    body: &[u8],
    limit: usize,
    reject_non_success: bool,
) -> Result<HttpResponse, TransportError> {
    let mut stream = TcpStream::connect(address)
        .await
        .map_err(|_| TransportError::Unavailable)?;
    let header = format!("POST /proxy HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {secret}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|_| TransportError::Unavailable)?;
    stream
        .write_all(body)
        .await
        .map_err(|_| TransportError::Unavailable)?;
    stream
        .flush()
        .await
        .map_err(|_| TransportError::Unavailable)?;
    read_response_with_status(&mut BufReader::new(stream), limit, reject_non_success).await
}

async fn read_response_with_status<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    limit: usize,
    reject_non_success: bool,
) -> Result<HttpResponse, TransportError> {
    let mut header_bytes = 0;
    let status_line = line(reader, 16 * 1024).await?;
    header_bytes += status_line.len() + 2;
    let status_line =
        std::str::from_utf8(&status_line).map_err(|_| TransportError::InvalidResponse)?;
    let mut parts = status_line.splitn(3, ' ');
    if !matches!(parts.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(TransportError::InvalidResponse);
    }
    let status = parts
        .next()
        .filter(|v| v.len() == 3 && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse::<u16>().ok())
        .ok_or(TransportError::InvalidResponse)?;
    if !(200..=599).contains(&status) {
        return Err(TransportError::InvalidResponse);
    }
    let mut length = None;
    let mut chunked = false;
    loop {
        let next = line(reader, 16 * 1024 - header_bytes).await?;
        header_bytes += next.len() + 2;
        if header_bytes > 16 * 1024 {
            return Err(TransportError::InvalidResponse);
        }
        if next.is_empty() {
            break;
        }
        let next = std::str::from_utf8(&next).map_err(|_| TransportError::InvalidResponse)?;
        let (name, value) = next
            .split_once(':')
            .ok_or(TransportError::InvalidResponse)?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(TransportError::InvalidResponse);
        }
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(TransportError::InvalidResponse);
            }
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| TransportError::TooLarge)?,
            );
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            if chunked || !value.eq_ignore_ascii_case("chunked") {
                return Err(TransportError::InvalidResponse);
            }
            chunked = true;
        }
    }
    if chunked && length.is_some() {
        return Err(TransportError::InvalidResponse);
    }
    if reject_non_success && !(200..300).contains(&status) {
        return Err(TransportError::Rejected { status });
    }
    let bytes = if chunked {
        read_chunked(reader, limit).await?
    } else if let Some(length) = length {
        if length > limit {
            return Err(TransportError::TooLarge);
        }
        let mut bytes = vec![0; length];
        reader
            .read_exact(&mut bytes)
            .await
            .map_err(|_| TransportError::InvalidResponse)?;
        bytes
    } else {
        let mut bytes = Vec::new();
        reader
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| TransportError::InvalidResponse)?;
        bytes
    };
    if bytes.len() > limit {
        return Err(TransportError::TooLarge);
    }
    let body = if (200..300).contains(&status) {
        validate_response(bytes, limit)?
    } else {
        bytes
    };
    Ok(HttpResponse { status, body })
}
#[cfg(test)]
async fn read_response<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    limit: usize,
) -> Result<Vec<u8>, TransportError> {
    legacy_response(read_response_with_status(reader, limit, true).await?)
}
async fn read_chunked<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    limit: usize,
) -> Result<Vec<u8>, TransportError> {
    let mut result = Vec::new();
    loop {
        let size = line(reader, 128).await?;
        let size = std::str::from_utf8(&size).map_err(|_| TransportError::InvalidResponse)?;
        // Go does not emit chunk extensions or trailers on this bridge.
        if size.is_empty() || !size.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(TransportError::InvalidResponse);
        }
        let size = usize::from_str_radix(size, 16).map_err(|_| TransportError::TooLarge)?;
        if size == 0 {
            if !line(reader, 2).await?.is_empty() {
                return Err(TransportError::InvalidResponse);
            }
            return Ok(result);
        }
        if size > limit.saturating_sub(result.len()) {
            return Err(TransportError::TooLarge);
        }
        let start = result.len();
        result.resize(start + size, 0);
        reader
            .read_exact(&mut result[start..])
            .await
            .map_err(|_| TransportError::InvalidResponse)?;
        let mut suffix = [0; 2];
        reader
            .read_exact(&mut suffix)
            .await
            .map_err(|_| TransportError::InvalidResponse)?;
        if suffix != *b"\r\n" {
            return Err(TransportError::InvalidResponse);
        }
    }
}
async fn line<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    limit: usize,
) -> Result<Vec<u8>, TransportError> {
    let mut bytes = Vec::new();
    loop {
        let part = reader
            .fill_buf()
            .await
            .map_err(|_| TransportError::InvalidResponse)?;
        if part.is_empty() {
            return Err(TransportError::InvalidResponse);
        }
        let end = part.iter().position(|b| *b == b'\n');
        let count = end.map_or(part.len(), |n| n + 1);
        if bytes.len() + count > limit {
            return Err(TransportError::InvalidResponse);
        }
        bytes.extend_from_slice(&part[..count]);
        reader.consume(count);
        if end.is_some() {
            if !bytes.ends_with(b"\r\n") {
                return Err(TransportError::InvalidResponse);
            }
            bytes.truncate(bytes.len() - 2);
            return Ok(bytes);
        }
    }
}

#[cfg(test)]
#[path = "http_status_tests.rs"]
mod status_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn http_rejections_keep_their_numeric_status() {
        for status in [401, 403, 404, 429, 500, 503] {
            let raw=format!("HTTP/1.1 {status} Rejected\r\nContent-Length: 999\r\n\r\n404 private diagnostics, deliberately not JSON");
            assert_eq!(
                read_response(&mut BufReader::new(raw.as_bytes()), 8 * 1024 * 1024).await,
                Err(TransportError::Rejected { status })
            );
        }
    }
    #[tokio::test]
    async fn framing_is_bounded_and_strict() {
        for raw in [
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
        ] {
            assert_eq!(
                read_response(&mut BufReader::new(raw.as_bytes()), 2)
                    .await
                    .unwrap(),
                b"{}"
            );
        }
        for raw in [
            "HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{} ",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{} \r\n0\r\n\r\n",
        ] {
            assert_eq!(
                read_response(&mut BufReader::new(raw.as_bytes()), 2).await,
                Err(TransportError::TooLarge)
            );
        }
        for raw in [
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{",
        ] {
            assert_eq!(
                read_response(&mut BufReader::new(raw.as_bytes()), 2).await,
                Err(TransportError::InvalidResponse)
            );
        }
        assert_eq!(
            read_response(
                &mut BufReader::new(
                    b"HTTP/1.1 302 Found\r\nLocation: http://example.invalid\r\n\r\n".as_slice()
                ),
                2
            )
            .await,
            Err(TransportError::Rejected { status: 302 })
        );
    }
}
