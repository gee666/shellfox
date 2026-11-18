use std::io;

pub const OUTPUT_CHUNK_BYTES: usize = 4096;
pub const OUTPUT_QUEUE_CHUNKS: usize = 64;

#[derive(Debug)]
pub enum PtyOutput {
    Data(Vec<u8>),
    Eof,
    ReadError(io::Error),
}
