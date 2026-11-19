//! Private, length-delimited stdio for one local WSL Files worker. No listener.
use crate::{
    AssetRequest, FileAsset, FileError, FileList, FileStat, ImageWrite, Location, ReadResult,
    TransferReply, TransferRequest, WriteResult, WriteText, LIST_BYTES, TRANSFER_IMAGE_BYTES,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const VERSION: u8 = 2;
const META_BYTES: usize = LIST_BYTES + 4096;
const BODY_BYTES: usize = TRANSFER_IMAGE_BYTES as usize;

#[derive(Debug, Deserialize, Serialize)]
#[serde(
    tag = "command",
    content = "input",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Request {
    List(Location),
    Open(Location),
    Save(WriteText),
    SaveImage(ImageWrite),
    Asset(AssetRequest),
    CreateDirectory(Location),
    Stat(Location),
    Transfer(TransferRequest),
    ReadInline(Location),
    WriteInline(WriteText),
    AssetInline(AssetRequest),
    WriteImageInline(ImageWrite),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(
    tag = "result",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Reply {
    Ready { version: u8, uid: u32 },
    List(FileList),
    Document(ReadResult),
    Saved(WriteResult),
    Asset(FileAsset),
    Stat(FileStat),
    Transfer(TransferReply),
    Error(FileError),
}

pub fn read<T: DeserializeOwned>(stream: &mut impl Read) -> io::Result<Option<(T, Vec<u8>)>> {
    let mut header = [0; 8];
    // Only EOF before a new header is a normal close.
    loop {
        match stream.read(&mut header[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    stream.read_exact(&mut header[1..])?;
    let meta = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let body = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    check(meta, body)?;
    let mut json = vec![0; meta];
    stream.read_exact(&mut json)?;
    let message = serde_json::from_slice(&json)?;
    let mut payload = vec![0; body];
    stream.read_exact(&mut payload)?;
    Ok(Some((message, payload)))
}

pub fn write(stream: &mut impl Write, message: &impl Serialize, payload: &[u8]) -> io::Result<()> {
    let meta = serde_json::to_vec(message)?;
    check(meta.len(), payload.len())?;
    stream.write_all(&(meta.len() as u32).to_le_bytes())?;
    stream.write_all(&(payload.len() as u32).to_le_bytes())?;
    stream.write_all(&meta)?;
    stream.write_all(payload)?;
    stream.flush()
}

fn check(meta: usize, body: usize) -> io::Result<()> {
    if meta == 0 || meta > META_BYTES || body > BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "WSL Files frame exceeds its limits",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_payload_and_bounded_frames() {
        let mut framed = Vec::new();
        let payload = b"\xef\xbb\xbfalpha\r\n\xe9\x9b\xaa";
        write(
            &mut framed,
            &Request::Open(Location { path: "/a".into() }),
            payload,
        )
        .unwrap();
        let mut stream = framed.as_slice();
        let (Request::Open(location), bytes) = read::<Request>(&mut stream).unwrap().unwrap()
        else {
            panic!("unexpected request")
        };
        assert_eq!(location.path, "/a");
        assert_eq!(bytes, payload);
        assert!(read::<Request>(&mut stream).unwrap().is_none());
        assert!(read::<Request>(&mut [1u8].as_slice()).is_err());
        assert!(read::<Request>(&mut [255u8; 8].as_slice()).is_err());
    }
}
