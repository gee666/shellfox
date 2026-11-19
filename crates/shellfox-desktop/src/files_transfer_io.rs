use super::{Transfer, TransferDirection};
use crate::files::{DocumentBackend, pending::basename};
use anyhow::{Context as _, Result, ensure};
use shellfox_files::EditorSession;
use shellfox_protocol as wire;
use std::{path::PathBuf, sync::Arc};

pub(super) async fn upload(
    backend: DocumentBackend,
    local: Arc<EditorSession>,
    folder: String,
    picked: PathBuf,
    state: Arc<Transfer>,
) -> Result<String> {
    let peer = backend
        .peer_backend()
        .context("Expected a peer destination.")?;
    let picked = picked.to_str().context("The selected path is not UTF-8.")?;
    let source = local.file_source(picked, state.cancel.clone())?;
    let name = basename(&source.header().path).to_owned();
    let size = source.header().size;
    state.start(name.clone(), size);
    let request = wire::BeginFileWrite {
        session_id: None,
        tab_id: None,
        path: destination(&folder, &name)?,
        size,
        mode: wire::FileWriteMode::Create,
        revision: None,
        content: wire::FileContent::File,
        mime_type: None,
    };
    peer.send_chunks(request, source, &state.cancel, |offset| {
        state.advance(offset);
        Ok(())
    })
    .await?;
    Ok(name)
}

pub(super) async fn download_header(
    backend: DocumentBackend,
    selected: String,
    state: Arc<Transfer>,
) -> Result<wire::FileReadTransfer> {
    let peer = backend.peer_backend().context("Expected a peer file.")?;
    let cursor = peer.begin_read(&selected, None, &state.cancel).await?;
    let header = cursor.header().clone();
    peer.close_read(&cursor).await;
    state.start(basename(&header.path).to_owned(), header.size);
    Ok(header)
}

pub(super) async fn download(
    backend: DocumentBackend,
    local: Arc<EditorSession>,
    header: wire::FileReadTransfer,
    picked: PathBuf,
    state: Arc<Transfer>,
) -> Result<String> {
    let peer = backend.peer_backend().context("Expected a peer source.")?;
    let picked = picked.to_str().context("The selected path is not UTF-8.")?;
    let mut sink = local.file_sink(picked, header.size, state.cancel.clone())?;
    let mut cursor = peer.begin_read(&header.path, None, &state.cancel).await?;
    let result = async {
        let current = cursor.header();
        ensure!(
            current.path == header.path
                && current.size == header.size
                && current.revision == header.revision,
            "The host file changed while choosing the download destination."
        );
        peer.receive_chunks(&mut cursor, &state.cancel, |bytes, offset| {
            if !bytes.is_empty() {
                sink.append(bytes)?;
            }
            state.advance(offset);
            Ok(())
        })
        .await?;
        let result = sink.finish()?;
        ensure!(
            result.size == header.size,
            "The local download boundary changed."
        );
        Ok(basename(&header.path).to_owned())
    }
    .await;
    peer.close_read(&cursor).await;
    result
}

fn destination(folder: &str, name: &str) -> Result<String> {
    ensure!(
        !name.is_empty() && !name.contains(['/', '\\', ':']) && !matches!(name, "." | ".."),
        "The selected filename contains unsupported characters."
    );
    let separator = if folder.contains('\\') && !folder.contains('/') {
        '\\'
    } else {
        '/'
    };
    let path = if folder.ends_with(['/', '\\']) {
        format!("{folder}{name}")
    } else {
        format!("{folder}{separator}{name}")
    };
    wire::files::path(&path).map_err(|_| anyhow::anyhow!("Invalid upload destination."))?;
    Ok(path)
}

pub(super) fn safe_name(direction: TransferDirection, name: &str) -> String {
    debug_assert_eq!(direction, TransferDirection::Download);
    let name: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_control() || "<>:\"/\\|?*".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let name = name.trim_end_matches(['.', ' ']);
    let first = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = matches!(first.as_str(), "con" | "prn" | "aux" | "nul")
        || first.len() == 4
            && matches!(&first.as_bytes()[..3], b"com" | b"lpt")
            && matches!(first.as_bytes()[3], b'1'..=b'9');
    if reserved {
        format!("download-{name}")
    } else if name.is_empty() {
        "download-file".into()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_paths_use_the_remote_separator_and_download_names_are_safe() {
        assert_eq!(
            destination(r"P:\remote", "file.bin").unwrap(),
            r"P:\remote\file.bin"
        );
        assert_eq!(
            destination("/remote/", "file.bin").unwrap(),
            "/remote/file.bin"
        );
        assert!(destination("/remote/", "../escaped").is_err());
        assert_eq!(
            safe_name(TransferDirection::Download, "Con.txt"),
            "download-Con.txt"
        );
    }
}
