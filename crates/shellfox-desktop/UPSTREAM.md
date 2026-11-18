# GPUI API and license review

The desktop uses the published `gpui = '=0.2.2'` crate, not Zed HEAD. Cargo.lock
pins the dependency graph. The inspected archive was
`https://static.crates.io/crates/gpui/gpui-0.2.2.crate`, SHA-256
`979b45cfa6ec723b6f42330915a1b3769b930d02b2d505f9697f8ca602bee707`.

Its edition is 2024 and its license is Apache-2.0. The original GPUI copyright
and complete license are retained in `notices/GPUI-LICENSE-APACHE`. Linking GPUI
does not license the whole Zed repository for reuse. No wider Zed application
code or xterm.js code was copied into this experiment.

API review used the packaged `examples/hello_world.rs`, `painting.rs`, `input.rs`
and `on_window_close_quit.rs`, plus `src/{app,window,input,text_system,interactive}.rs`,
`src/app/context.rs`, `src/platform/keystroke.rs` and
`src/platform/windows/events.rs`. The implementation uses normal GPUI window,
canvas, shaped-text, focus and entity input-handler APIs. In particular,
Windows translates unhandled printable keydown into native committed text, so
this adapter does not also encode the same plain keydown. Modified keys and
special keys take the terminal encoder route.

These API checks do not prove native IME, AltGr, Option composition or physical
printable-key metadata parity. Those limits are listed in
`docs/gpui-terminal-experiment.md` at the repository root. Dependency-wide
license and security review is not complete, and this experiment is not wired
into a release packager.
