use crate::{error, file_ops};
use shellfox_files as files;
use shellfox_protocol::{self as wire, AppError, ErrorCode, RemoteCommand, Validate};

pub fn decode(command: &RemoteCommand) -> Result<files::TransferRequest, AppError> {
    command
        .validate()
        .map_err(|_| error(ErrorCode::Validation, "Invalid transfer request.", false))?;
    use files::TransferRequest as T;
    use wire::RemoteCommand::*;
    Ok(match command {
        BeginFileRead(input) => T::BeginRead(files::BeginRead {
            path: input.path.clone(),
            asset_path: input.asset_path.clone(),
        }),
        BeginFileWrite(input) => T::BeginWrite(files::BeginWrite {
            path: input.path.clone(),
            size: input.size,
            mode: match input.mode {
                wire::FileWriteMode::Create => files::WriteMode::Create,
                wire::FileWriteMode::Replace => files::WriteMode::Replace,
            },
            revision: input.revision.clone(),
            content: match input.content {
                wire::FileContent::File => files::TransferContent::File,
                wire::FileContent::Text => files::TransferContent::Text,
                wire::FileContent::Image => files::TransferContent::Image,
            },
            mime_type: input.mime_type.map(|mime| {
                match mime {
                    wire::ImageWriteMime::Png => "image/png",
                    wire::ImageWriteMime::Jpeg => "image/jpeg",
                    wire::ImageWriteMime::Webp => "image/webp",
                }
                .to_owned()
            }),
        }),
        ReadFileChunk(input) => T::Read(files::ReadChunk {
            transfer_id: input.transfer_id,
            offset: input.offset,
        }),
        WriteFileChunk(input) => T::Write(files::WriteChunk {
            transfer_id: input.transfer_id,
            offset: input.offset,
            data: wire::decode_chunk(&input.data)
                .map_err(|_| error(ErrorCode::Validation, "Invalid file chunk encoding.", false))?,
        }),
        FinishFileWrite(input) => T::Finish(files::TransferIdentity {
            transfer_id: input.transfer_id,
        }),
        CancelFileTransfer(input) => T::Cancel(files::TransferIdentity {
            transfer_id: input.transfer_id,
        }),
        _ => return Err(crate::capability_unavailable("not a file transfer command")),
    })
}

pub fn encode(reply: files::TransferReply) -> Result<serde_json::Value, AppError> {
    use files::TransferReply::*;
    match reply {
        ReadStarted(result) => file_ops::checked(wire::FileReadTransfer {
            transfer_id: result.transfer_id,
            path: result.path,
            size: result.size,
            revision: result.revision,
            kind: match result.kind {
                files::ContentKind::Text => wire::FileReadKind::Text,
                files::ContentKind::Image => wire::FileReadKind::Image,
                files::ContentKind::Binary => wire::FileReadKind::Binary,
            },
            mime_type: result.mime_type,
            chunk_bytes: result.chunk_bytes,
        }),
        WriteStarted(result) => file_ops::checked(wire::FileWriteTransfer {
            transfer_id: result.transfer_id,
            chunk_bytes: result.chunk_bytes,
        }),
        Chunk(result) => file_ops::checked(wire::FileChunk {
            offset: result.offset,
            data: wire::encode_chunk(&result.data)
                .map_err(|_| error(ErrorCode::Internal, "Invalid file chunk size.", false))?,
            eof: result.eof,
        }),
        Progress(result) => file_ops::checked(wire::TransferOffset {
            offset: result.offset,
        }),
        Finished(result) => file_ops::checked(wire::FileTransferResult {
            path: result.path,
            size: result.size,
            revision: result.revision,
        }),
        Cancelled => file_ops::checked(wire::Cancelled { cancelled: true }),
    }
}
