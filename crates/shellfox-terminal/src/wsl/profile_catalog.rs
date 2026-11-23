use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
    thread,
};

#[cfg(windows)]
use super::{interactive_distro_list, OsWsl, WslError};

const READY: &str = "SHELLFOX_PIDFD_READY_V1";

#[cfg(windows)]
impl OsWsl {
    /// Fixed, read-only check: never start an interactive shell, install anything,
    /// or persist guest state. Only the authorized owner starts this catalog.
    pub fn probe_interactive_profiles(
        &self,
        cancelled: impl Fn() -> bool + Sync,
    ) -> Result<Vec<(String, bool)>, WslError> {
        use super::super::discovery_process::profile_command;
        let list = profile_command(self.command(&["--list", "--quiet"]), &cancelled)?;
        let names = interactive_distro_list(&list)?;
        let script =
            include_str!("../../../../native/wsl/profile-preflight.sh").replace("\r\n", "\n");
        let results = collect_profiles(names, &cancelled, |name| {
            let command = self.command(&[
                "--distribution",
                name,
                "--exec",
                "/bin/sh",
                "-c",
                &script,
                "shellfox-preflight",
            ]);
            profile_command(command, &cancelled).is_ok_and(|bytes| ready(&bytes))
        });
        if cancelled() {
            return Err(WslError::Unavailable);
        }
        Ok(results)
    }
}

fn ready(bytes: &[u8]) -> bool {
    let text = if bytes.contains(&0) {
        super::decode_utf16(bytes).ok()
    } else {
        String::from_utf8(bytes.to_vec()).ok()
    };
    text.is_some_and(|text| text.trim_start_matches('\u{feff}').replace('\r', "").trim() == READY)
}

fn collect_profiles(
    names: Vec<String>,
    cancelled: impl Fn() -> bool + Sync,
    probe: impl Fn(&str) -> bool + Sync,
) -> Vec<(String, bool)> {
    let head = AtomicUsize::new(0);
    let results: Vec<_> = names.iter().map(|_| OnceLock::new()).collect();
    thread::scope(|scope| {
        for _ in 0..names.len().min(4) {
            scope.spawn(|| {
                while !cancelled() {
                    let i = head.fetch_add(1, Ordering::Relaxed);
                    let Some(name) = names.get(i) else {
                        break;
                    };
                    let _ = results[i].set(probe(name));
                }
            });
        }
    });
    names
        .into_iter()
        .zip(
            results
                .into_iter()
                .map(|result| result.into_inner().unwrap_or(false)),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn exact_ready_marker_accepts_wsl_encodings_but_not_partial_or_extra_output() {
        assert!(ready(b"SHELLFOX_PIDFD_READY_V1\n"));
        let utf16: Vec<_> = format!("\u{feff}{READY}\r\n")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(ready(&utf16));
        assert!(!ready(b"prefix SHELLFOX_PIDFD_READY_V1"));
        assert!(!ready(b"SHELLFOX_PIDFD_READY_V1\nextra"));
        assert!(!ready(&[0, 1, 2]));
    }

    #[test]
    fn four_workers_refill_and_preserve_installation_order_including_failures() {
        let first_batch = AtomicUsize::new(0);
        let names = (0..9).map(|n| n.to_string()).collect();
        let results = collect_profiles(
            names,
            || false,
            |name| {
                let n: usize = name.parse().unwrap();
                if n < 4 {
                    first_batch.fetch_add(1, Ordering::Release);
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while first_batch.load(Ordering::Acquire) < 4 {
                        assert!(Instant::now() < deadline, "first wave must not be serial");
                        thread::sleep(Duration::from_millis(1));
                    }
                }
                n != 3 && n != 6
            },
        );
        assert_eq!(
            results,
            (0..9)
                .map(|n| (n.to_string(), n != 3 && n != 6))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            collect_profiles(vec!["closed".into()], || true, |_| panic!("cancelled")),
            vec![("closed".into(), false)]
        );
    }
}
