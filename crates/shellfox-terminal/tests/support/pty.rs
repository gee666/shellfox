use shellfox_terminal::{PtyCommand, PtyConfig, PtyOutput, PtySession, OUTPUT_CHUNK_BYTES};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&root).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Wall-clock timestamps alone can collide between parallel tests.
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(
            "terminal-test-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(std::path::absolute(path).unwrap())
    }
    pub fn config(&self, mode: &str) -> PtyConfig {
        let mut config = PtyConfig::new(&self.0);
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-pty-test-helper"));
        config.supervisor = Some(executable.clone());
        config.command = Some(PtyCommand {
            program: executable.into_os_string(),
            args: vec![mode.into()],
        });
        config
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0)
            .unwrap_or_else(|error| panic!("remove owned scratch {}: {error}", self.0.display()));
    }
}

pub fn drain(session: &mut PtySession) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut output = Vec::new();
    loop {
        assert!(
            Instant::now() < deadline,
            "PTY output did not finish: {}",
            String::from_utf8_lossy(&output)
        );
        session.try_wait().unwrap();
        match session.try_read() {
            Ok(Some(PtyOutput::Data(bytes))) => {
                assert!(bytes.len() <= OUTPUT_CHUNK_BYTES);
                output.extend(bytes);
            }
            Ok(Some(PtyOutput::Eof)) => return output,
            Ok(Some(PtyOutput::ReadError(error))) => panic!("PTY read: {error}"),
            Ok(None) => thread::sleep(Duration::from_millis(2)),
            Err(error) => panic!("missing final EOF: {error}"),
        }
    }
}
