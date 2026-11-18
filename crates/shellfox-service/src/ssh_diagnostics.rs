//! Pre-auth public-protocol classification only. Never retain output, paths,
//! prompts, credentials or arbitrary child text. The PTY merges stdout/stderr;
//! these are observed text categories, not authenticated root-cause claims.
const LIMIT: usize = 64 * 1024;
const PATTERNS: [(&[u8], &str); 15] = [
    (b"unknown option", "program option rejected"),
    (
        b"bad configuration option",
        "program configuration option rejected",
    ),
    (
        b"can't open user config file",
        "explicit configuration file unavailable",
    ),
    (
        b"could not open user config file",
        "explicit configuration file unavailable",
    ),
    (
        b"no such file or directory",
        "program reported a missing path",
    ),
    (
        b"host key verification failed",
        "host-key verification rejected",
    ),
    (
        b"remote host identification has changed",
        "host-key change reported",
    ),
    (b"unable to negotiate", "transport negotiation rejected"),
    (
        b"unsupported kex method",
        "unsupported key-exchange implementation reported",
    ),
    (b"ssh_askpass", "askpass helper diagnostic observed"),
    (b"load key", "identity loader diagnostic observed"),
    (b"bad permissions", "file permissions diagnostic observed"),
    (
        b"permission denied",
        "access/authentication rejection text observed",
    ),
    (b"connection refused", "connection refusal reported"),
    (b"exec request failed", "remote command request rejected"),
];
pub(super) struct PublicDiagnostics {
    prefixes: [usize; PATTERNS.len()],
    found: [bool; PATTERNS.len()],
    bytes: usize,
}
impl Default for PublicDiagnostics {
    fn default() -> Self {
        Self {
            prefixes: [0; PATTERNS.len()],
            found: [false; PATTERNS.len()],
            bytes: 0,
        }
    }
}
impl PublicDiagnostics {
    pub fn observe(&mut self, bytes: &[u8]) {
        for byte in bytes.iter().take(LIMIT - self.bytes) {
            let byte = byte.to_ascii_lowercase();
            for (index, (pattern, _)) in PATTERNS.iter().enumerate() {
                let matched = self.prefixes[index];
                self.prefixes[index] = if byte == pattern[matched] {
                    matched + 1
                } else {
                    usize::from(byte == pattern[0])
                };
                if self.prefixes[index] == pattern.len() {
                    self.found[index] = true;
                    self.prefixes[index] = 0;
                }
            }
            self.bytes += 1;
        }
    }
    pub fn summary(&self) -> String {
        let categories = PATTERNS
            .iter()
            .zip(self.found)
            .filter_map(|((_, label), found)| found.then_some(*label))
            .collect::<Vec<_>>();
        format!("pre-auth merged-output bytes observed={}{}; public diagnostic categories: {}; original runtime closed, exit code unavailable",self.bytes,if self.bytes==LIMIT{" (capture limit reached)"}else{""},if categories.is_empty(){"none recognized".into()}else{categories.join(", ")})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_public_patterns_classify_without_retaining_private_text() {
        let mut d = PublicDiagnostics::default();
        d.observe(b"PRIVATE-SENTINEL C:/private/name ssh_ask");
        d.observe(b"pass: exec: No such file or directory\r\n");
        let result = d.summary();
        assert!(result.contains("askpass helper diagnostic observed"));
        assert!(result.contains("program reported a missing path"));
        assert!(!result.contains("PRIVATE-SENTINEL"));
        assert!(!result.contains("C:/private/name"));
    }
    #[test]
    fn diagnostics_are_bounded_and_do_not_infer_authentication_or_exit() {
        let mut d = PublicDiagnostics::default();
        d.observe(&vec![b'x'; LIMIT + 1024]);
        d.observe(b"Permission denied");
        let result = d.summary();
        assert!(result.contains("65536 (capture limit reached)"));
        assert!(result.contains("none recognized"));
        assert!(result.contains("exit code unavailable"));
        assert!(!result.contains("access/authentication rejection"));
    }
}
