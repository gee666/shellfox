# Ghostty recovery contracts

`FormattedActiveBufferV1` is a negotiated ANSI projection, not complete terminal state. It is separate from `VisibleScreen`, which only repaints visible cells. Neither capability may be labeled `FullState`. Selection and viewport scroll position are view-local and are not restored by this projection.

## Implemented projection

`crates/shellfox-runtime/src/recovery.rs` builds the response without feeding the source terminal, querying its PTY, or draining its pending replies:

1. Copy the public replay-safe continuation. Refuse recovery if tracking was unavailable or exceeded its bound.
2. Ask the public formatter for active-buffer VT data, with trimming and unwrapping disabled. Include supported public extras except the process-controlled working-directory OSC 7 value.
3. Prefix CAN and RIS. CAN cancels an unfinished receiver sequence; RIS resets stale cells, history and modes even when the grid size is unchanged. Completing an old OSC with ST is deliberately avoided.
4. Append the continuation last. No SGR, cursor or other command may follow an unfinished parser suffix before the next ordered live byte.
5. Refuse output above 4 MiB, without shortening history or returning a partial stream. The formatter and Rust allocations must enforce the bound before allocating an unbounded native result.

The sender serializes this operation with output and resize. Its response sequence is the highest complete source chunk represented by the projection, including any unfinished parser suffix within that chunk. Later chunks start strictly after this boundary. The receiver replaces its parser state and ACKs the recovery boundary before delivery resumes. Recovery does not implicitly acknowledge outstanding output or authorize input. Eirik's runtime delivery module owns that sequence and credit integration.

The receiving renderer is a passive mirror. Query replies and clipboard operations must remain disabled during replay and recovery, and the authoritative host alone answers live terminal queries. Appending a continuation can leave a query or OSC unfinished; completing it with later live bytes must not turn a mirror into another authoritative host. Links are untrusted metadata and still require the normal URL/user-intent policy. Recovery never grants permission to open a link.

## Why this is not full state

These facts are from the retained official libghostty-vt 1.0.1 source tarball, whose hash and signed release manifest are recorded in `crates/ghostty-vt-sys/README.md`:

- `include/ghostty/vt/formatter.h`, `ghostty_formatter_terminal_new`, describes a formatter for the terminal's **active screen**. The public constructor has no screen argument.
- `src/terminal/formatter.zig`, `TerminalFormatter`, explicitly says it only emits the active screen. Formatting all screens requires the internal Zig `ScreenFormatter`; that is not a public C constructor.
- The public terminal getter list has no saved-cursor export or immutable inactive-screen formatter.
- `ScreenFormatter.format`, pending-wrap handling, explicitly says the blank-cell workaround replaces the saved cursor, which the formatter does not emit.
- The formatter emits current Kitty keyboard flags, not its complete push/pop stack. Current modes are emitted only when they differ from Ghostty's configured reset defaults. A formatter result therefore does not prove all future mode-stack operations will behave identically on another engine.
- Public continuation tracking is disabled by default. It must be enabled before the first source write. Enabling it after an unfinished sequence cannot reconstruct missing earlier bytes. `ghostty_terminal_continuation_buf` reports unavailable state rather than returning a guessed prefix.

The regression tests demonstrate two concrete losses: returning from alternate screen cannot restore the source's inactive primary buffer, and DECRC cannot restore an omitted DECSC position. These are counterexamples to a full-state claim, not accepted equivalence tests. Switching or replaying control sequences into the authoritative source to inspect hidden state is not an acceptable workaround.

## Complete migration path

There are two different compatibility requirements. A supported new Ghostty client can use a separately negotiated native recovery format, but an ordinary CLI terminal or legacy xterm cannot decode it.

For new Ghostty clients, add a versioned snapshot capability only after defining and testing all of the following:

- Pin the library version and record the snapshot format version independently. C ABI 1.x stability is not snapshot-byte compatibility.
- Validate the envelope, dimensions, byte size, continuation bound, format version and library compatibility before decoding. Use a bounded public decoder. Unknown or unsupported versions must fail closed or select another explicitly offered capability.
- Decode into an isolated terminal with no host callbacks or external effects. Bound the input reader, decoded row/image counts and resident allocation budget, not just the encoded payload length. Apply the public decoder's maximum-continuation, history-compression and Kitty-image-storage options explicitly. Its defaults are not Shellfox's resource policy. Publish the replacement only after complete successful validation and a sequence-barrier commit. Reject trailing unconsumed bytes. Never send snapshot bytes to an ANSI renderer.
- Test inactive buffers, both saved cursors, keyboard stacks, tab stops, margins, wrap state, graphics, selection and unfinished UTF-8/VT data, plus malformed/version-mismatched input and decode failure without replacing the current view.

This is a proposed protocol, not an implemented or accepted snapshot path.

For transitional xterm/CLI clients, retain the current legacy host's ANSI restore implementation and its regression suites while its host/client protocol remains active. Do not delete it because `FormattedActiveBufferV1` exists. A complete replacement requires either an upstream public immutable full-ANSI export, or a separately specified renderer-independent state protocol with a tested legacy ANSI adapter. Keep the old engine at the existing transition boundary; do not create a second query-authoritative parser attached to the new PTY. Full migration acceptance remains blocked until the replacement preserves the required state or the user explicitly approves a narrower recovery contract.

## Verification

The recovery tests are original differential fixtures. They use the existing Shellfox screen-mirror tests as behavior requirements, not copied xterm implementation code. No upstream code is translated into a second parser.

Run the Rust recovery tests with the repository's installed Node and `@xterm/headless` dependency available. They start no PTY or shell session. Node subprocesses have an internal ten-second watchdog. Ghostty tests compare owned cell styles, cursor, full active-buffer selection text and post-recovery live suffixes. Legacy comparisons use xterm's public buffer and mode API. No production data is opened.

Verified on Windows x86_64 MSVC with the pinned libghostty-vt 1.0.1 DLL:

- `cargo test --locked -p shellfox-runtime recovery::tests -- --nocapture`: 12 passed.
- `cargo test --locked --release -p shellfox-runtime recovery::tests -- --nocapture`: 12 passed.
- `cargo test --locked -p shellfox-runtime --lib`: all 28 runtime unit tests passed, including recovery and sequence/credit tests.
- `pnpm exec vitest run src/main/terminal/screen-mirror.test.ts src/main/terminal/screen-mirror-lifecycle.test.ts`: 9 passed.
- `pnpm typecheck`: passed, including mobile TypeScript.
- Direct `rustfmt --check` on `recovery.rs` and `recovery_tests.rs`: passed.
- `cargo clippy --locked -p shellfox-runtime --all-targets -- -D warnings`: passed after the concurrent runtime wiring stabilized. The previously documented vendored portable-pty unused-constant warning remains a dependency warning.

The twelve tests include the three negative full-state counterexamples, continuation-limit refusal, bounded-builder refusal without partial output, same-grid repaint, scrollback, resize, margins/origin, pending wrap, styles and live suffixes. Eight xterm comparison invocations run across the positive fixtures, including each incomplete UTF-8/CSI case. The old malformed receiver OSC 52 is cancelled without invoking its clipboard handler. The projection does not replay completed clipboard or working-directory effects.

No macOS, Linux, Android or iOS runtime execution is claimed by these Windows tests. No PTY, host transport or actual mobile/desktop recovery delivery was exercised here. Eirik owns the runtime capability/sequence integration, and the service must preserve and check the selected capability rather than strip it or send the projection implicitly to legacy clients.
