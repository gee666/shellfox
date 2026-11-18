use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

fn emit(value: serde_json::Value) {
    println!("{value}");
    std::io::stdout().flush().unwrap();
}
fn hold_process_lease(directory: &Path) {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options.open(directory.join("process-lease")).unwrap();
    #[cfg(unix)]
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    // Test witness closes only at actual process destruction, never Rust scope exit.
    std::mem::forget(file);
    std::fs::write(
        directory.join("original-pid"),
        std::process::id().to_string(),
    )
    .unwrap();
}
fn gate(directory: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !directory.join("release").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}
pub fn probe_hold(executable: &Path) {
    if executable.file_stem().is_some_and(|s| s == "probe-hold") {
        let directory = executable.parent().unwrap();
        hold_process_lease(directory);
        gate(directory);
    }
}
pub fn ownership(directory: &Path) {
    let name = directory.file_name().unwrap().to_string_lossy();
    let controlled = name.starts_with("ownership-");
    if controlled {
        hold_process_lease(directory);
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("acquisitions"))
            .unwrap()
            .write_all(b"1\n")
            .unwrap();
    }
    emit(json!({"locked":true}));
    std::io::copy(&mut std::io::stdin(), &mut std::io::sink()).unwrap();
    if controlled {
        std::fs::write(directory.join("stdin-closed-once"), b"1").unwrap();
        if name == "ownership-error" {
            eprintln!("synthetic ownership diagnostic");
        }
        gate(directory);
    }
}
pub fn started(mode: &str, directory: &Path) {
    if mode.starts_with("stop-") || mode.starts_with("start-") {
        hold_process_lease(directory);
    }
    if mode == "start-hold" {
        gate(directory);
    }
}
pub fn stop(mode: &str, directory: &Path, id: &str) -> bool {
    if !mode.starts_with("stop-") {
        return false;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("stop-attempts"))
        .unwrap()
        .write_all(b"1\n")
        .unwrap();
    match mode {
        "stop-ack-live" => {
            emit(json!({"id":id,"ok":true,"value":{"stopped":true}}));
            gate(directory);
        }
        "stop-eof-live" => {
            close_stdout();
            gate(directory);
        }
        "stop-lost-exit" => {}
        _ => panic!("unknown synthetic stop case"),
    }
    true
}
fn close_stdout() {
    // SAFETY: this fixture deliberately closes its own stdout pipe once. No
    // further output is written, and no other process handle is involved.
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        drop(unsafe { File::from_raw_fd(1) });
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        drop(unsafe { File::from_raw_handle(std::io::stdout().as_raw_handle()) });
    }
}
