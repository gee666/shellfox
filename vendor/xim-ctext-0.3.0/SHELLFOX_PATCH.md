# xim-ctext 0.3.0 patch

Upstream crates.io archive SHA-256 (matches Cargo.lock before the path override):
`2ac61a7062c40f3c37b6e82eeeef835d5cc7824b632a72784a89b3963c33284c`.

Upstream decoded only a leading UTF-8 segment and ISO-2022-JP. XIM servers
(ibus-x11 and others via Xlib) send Cyrillic, Greek, Latin-1 and CJK as ISO 2022
segments, so the decoder returned an error, and zed-xim's `expect` crashed the
GPUI desktop on Linux X11 with the first Russian key.

`src/lib.rs` now decodes full COMPOUND_TEXT and replaces unknown parts with
U+FFFD. `src/tests.rs` holds real Xlib output for every script. The encoder and
the public API are unchanged, except that `DecodeError` carries the lossy text
and `compound_text_to_utf8_lossy` was added. `LICENSE` is the MIT license of
the upstream repository (Riey/xim-rs), which the crate archive omitted.
