use shellfox_terminal::raw_process::{RawCommand, RawOutput, RawProcess, RawStream};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub(super) struct Fixture {
    pub(super) directory: PathBuf,
    pub(super) helper: PathBuf,
}
impl Fixture {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tmp");
        let directory = root.join(format!(
            "raw-process-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        Self {
            directory,
            helper: PathBuf::from(env!("CARGO_BIN_EXE_shellfox-raw-test-helper")),
        }
    }
    pub(super) fn command(&self, args: &[&str]) -> RawCommand {
        RawCommand {
            executable: self.helper.to_str().unwrap().into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            cwd: self.directory.to_str().unwrap().into(),
            env: BTreeMap::new(),
        }
    }
    pub(super) fn spawn(&self, args: &[&str]) -> RawProcess {
        RawProcess::spawn(&self.helper, self.command(args)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            match std::fs::remove_dir_all(&self.directory) {
                Ok(()) => break,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) if !std::thread::panicking() => {
                    panic!("Owned fixture cleanup failed: {error}")
                }
                Err(_) => break,
            }
        }
    }
}
pub(super) fn collect(process: &mut RawProcess) -> (Vec<u8>, Vec<u8>, i32) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut eof = [false; 2];
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let code = process.poll_exit().unwrap();
        while let Some(output) = process.try_output() {
            match output {
                RawOutput::Data { stream, bytes } => {
                    if stream == RawStream::Stdout {
                        stdout.extend(bytes);
                    } else {
                        stderr.extend(bytes);
                    }
                }
                RawOutput::Eof(stream) => eof[usize::from(stream == RawStream::Stderr)] = true,
                RawOutput::Failed(stream) => panic!("Fixture pipe failed: {stream:?}"),
            }
        }
        if let Some(code) = code.filter(|_| eof == [true; 2]) {
            process.close().unwrap();
            return (stdout, stderr, code);
        }
        assert!(
            Instant::now() < deadline,
            "Owned raw process failed to finish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
