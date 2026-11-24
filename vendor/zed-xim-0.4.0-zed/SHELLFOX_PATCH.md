# zed-xim 0.4.0-zed patch

Upstream crates.io archive SHA-256 (matches Cargo.lock before the path override):
`0c0b46ed118eba34d9ba53d94ddc0b665e0e06a2cf874cfa2dd5dec278148642`.

Only `src/client.rs` changes: the three `compound_text_to_utf8(..).expect(..)`
calls for commit, preedit draw and reset replies now log a warning and use the
lossy text, so text from an XIM server can never panic the client. The decoder
itself is fixed in the patched `xim-ctext` (vendor/xim-ctext-0.3.0).
