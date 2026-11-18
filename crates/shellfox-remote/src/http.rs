use shellfox_transport::incoming::SignedHeaders;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

pub(crate) struct Request {
    pub path: String,
    pub body: Vec<u8>,
    pub peer: String,
    pub timestamp: String,
    pub signature: String,
}
impl Request {
    pub fn manager_endpoint(&self) -> Option<shellfox_protocol::Endpoint> {
        use shellfox_protocol::Endpoint;
        match self.path.as_str() {
            "/v2/discover" => Some(Endpoint::DiscoverV2),
            "/v2/pair" => Some(Endpoint::PairV2),
            "/v2/request" => Some(Endpoint::RequestV2),
            "/v2/events" => Some(Endpoint::EventsV2),
            "/v2/disconnect" => Some(Endpoint::DisconnectV2),
            _ => None,
        }
    }
    pub fn signed_headers(&self) -> SignedHeaders<'_> {
        SignedHeaders {
            peer: &self.peer,
            timestamp: &self.timestamp,
            signature: &self.signature,
        }
    }
}
/// One POST per connection. No chunked requests, pipelines, duplicate headers,
/// folded headers, URL normalization or JSON parsing before authentication.
pub(crate) async fn read(stream: &mut TcpStream) -> Result<Request, ()> {
    let mut reader = BufReader::new(stream);
    let first = line(&mut reader, 512).await?;
    let parts: Vec<_> = first.split(' ').collect();
    if parts.len() != 3
        || parts[0] != "POST"
        || parts[2] != "HTTP/1.1"
        || ![
            "/v1/discover",
            "/v1/pair",
            "/v1/request",
            "/v1/events",
            "/v1/disconnect",
            "/v2/discover",
            "/v2/pair",
            "/v2/request",
            "/v2/events",
            "/v2/disconnect",
        ]
        .contains(&parts[1])
    {
        return Err(());
    }
    let path = parts[1].to_owned();
    let mut headers = std::collections::HashMap::new();
    let mut bytes = first.len();
    loop {
        let header = line(&mut reader, 4096).await?;
        bytes += header.len() + 2;
        if bytes > 16 * 1024 || headers.len() > 32 {
            return Err(());
        }
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').ok_or(())?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(());
        }
        let name = name.to_ascii_lowercase();
        let value = value.trim_matches(' ');
        if value.chars().any(char::is_control) || headers.insert(name, value.to_owned()).is_some() {
            return Err(());
        }
    }
    if headers.contains_key("transfer-encoding") || headers.contains_key("expect") {
        return Err(());
    }
    let length = headers.get("content-length").ok_or(())?;
    if length.is_empty() || !length.bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    let length: usize = length.parse().map_err(|_| ())?;
    if length > shellfox_protocol::MAX_REQUEST_BYTES {
        return Err(());
    }
    let peer = headers.remove("x-shellfox-peer").ok_or(())?;
    let timestamp = headers.remove("x-shellfox-timestamp").ok_or(())?;
    let signature = headers.remove("x-shellfox-signature").ok_or(())?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await.map_err(|_| ())?;
    Ok(Request {
        path,
        body,
        peer,
        timestamp,
        signature,
    })
}
async fn line<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    limit: usize,
) -> Result<String, ()> {
    let mut bytes = Vec::new();
    loop {
        let part = reader.fill_buf().await.map_err(|_| ())?;
        if part.is_empty() {
            return Err(());
        }
        let end = part.iter().position(|b| *b == b'\n');
        let count = end.map_or(part.len(), |p| p + 1);
        if bytes.len() + count > limit {
            return Err(());
        }
        bytes.extend_from_slice(&part[..count]);
        reader.consume(count);
        if end.is_some() {
            if !bytes.ends_with(b"\r\n") {
                return Err(());
            }
            bytes.truncate(bytes.len() - 2);
            return String::from_utf8(bytes).map_err(|_| ());
        }
    }
}
pub(crate) async fn reply(stream: &mut TcpStream, body: &[u8]) -> Result<(), ()> {
    reply_with_status(stream, 200, body).await
}
pub(crate) async fn reply_with_status(
    stream: &mut TcpStream,
    status: u16,
    body: &[u8],
) -> Result<(), ()> {
    if !(200..=599).contains(&status) {
        return Err(());
    }
    if body.len() > shellfox_protocol::MAX_RESPONSE_BYTES {
        return Err(());
    }
    let reason = if status == 200 { "OK" } else { "Shellfox" };
    let header = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len());
    stream.write_all(header.as_bytes()).await.map_err(|_| ())?;
    stream.write_all(body).await.map_err(|_| ())?;
    stream.shutdown().await.map_err(|_| ())
}
