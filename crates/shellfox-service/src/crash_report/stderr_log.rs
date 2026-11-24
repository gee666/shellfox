//! Stderr goes through a pipe to a size-bounded file. A pipe, not the file
//! itself, because `eprintln!` panics when a write fails, and a full disk
//! must not crash the app; the pump just drops what it cannot store.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const LOG_LIMIT: u64 = 4 << 20;
const TAIL_BYTES: u64 = 256 << 10;

pub(super) fn capture(dir: &Path, role: &str) -> std::io::Result<()> {
    create_private_dir(dir)?;
    let log = dir.join(format!("{role}.log"));
    let (reader, writer) = std::io::pipe()?;
    // A terminal or test that started the app still sees the output.
    let inherited = original_stderr();
    redirect(writer)?;
    std::thread::Builder::new()
        .name(format!("{role}-log"))
        .spawn(move || pump(reader, &log, inherited))?;
    Ok(())
}

fn pump(mut reader: std::io::PipeReader, log: &Path, mut inherited: Option<File>) {
    let mut file = open(log);
    let mut size = file
        .as_ref()
        .and_then(|file| file.metadata().ok())
        .map_or(0, |metadata| metadata.len());
    let mut buffer = [0; 8192];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        // Errors are dropped: a closed journal stream must not stop the log.
        if inherited
            .as_mut()
            .is_some_and(|stderr| stderr.write_all(&buffer[..read]).is_err())
        {
            inherited = None;
        }
        if size >= LOG_LIMIT {
            drop(file.take());
            let _ = std::fs::rename(log, rotated(log));
            file = open(log);
            size = 0;
        }
        if file
            .as_mut()
            .is_some_and(|open| open.write_all(&buffer[..read]).is_ok())
        {
            size += read as u64;
        }
    }
}

fn open(log: &Path) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(log).ok()
}

fn rotated(log: &Path) -> PathBuf {
    let mut name = log.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

/// The last `lines` lines of the log, reaching into the rotated file when
/// the current one is short.
pub(super) fn tail(log: &Path, lines: usize) -> String {
    let mut text = String::new();
    for path in [rotated(log), log.to_owned()] {
        text.push_str(&read_end(&path));
    }
    let mut kept: Vec<_> = text.lines().rev().take(lines).collect();
    kept.reverse();
    kept.join("\n")
}

fn read_end(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn original_stderr() -> Option<File> {
    use std::os::fd::AsFd;
    std::io::stderr()
        .as_fd()
        .try_clone_to_owned()
        .ok()
        .map(File::from)
}

#[cfg(unix)]
fn redirect(writer: std::io::PipeWriter) -> std::io::Result<()> {
    rustix::stdio::dup2_stderr(&writer)?;
    Ok(())
}

#[cfg(windows)]
fn original_stderr() -> Option<File> {
    use std::os::windows::io::AsHandle;
    std::io::stderr()
        .as_handle()
        .try_clone_to_owned()
        .ok()
        .map(File::from)
}

/// Rust's stderr looks the handle up on every write, so swapping it is enough.
#[cfg(windows)]
fn redirect(writer: std::io::PipeWriter) -> std::io::Result<()> {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};
    let handle = writer.into_raw_handle();
    // SAFETY: the handle is a valid pipe end that stays open for the process.
    if unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_spans_the_rotated_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("ui.log");
        std::fs::write(rotated(&log), "old 1\nold 2\n").unwrap();
        std::fs::write(&log, "new 1\nnew 2\n").unwrap();
        assert_eq!(tail(&log, 3), "old 2\nnew 1\nnew 2");
        assert_eq!(tail(&dir.path().join("none.log"), 3), "");
    }

    #[test]
    fn the_pump_rotates_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("ui.log");
        std::fs::write(&log, vec![b'x'; LOG_LIMIT as usize]).unwrap();
        let (reader, mut writer) = std::io::pipe().unwrap();
        writer.write_all(b"fresh\n").unwrap();
        drop(writer);
        pump(reader, &log, None);
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "fresh\n");
        assert_eq!(std::fs::metadata(rotated(&log)).unwrap().len(), LOG_LIMIT);
    }
}
