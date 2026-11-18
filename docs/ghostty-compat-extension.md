# Pinned libghostty-vt compatibility extension

This is an additive Shellfox compatibility extension to the official libghostty-vt `vt-1.0.1` source. It is not a framework change, an official upstream release, or a universal FullState claim. Existing staged mobile libraries remain their original snapshot artifacts.

## Ownership and patch order

Apply exactly one current manifest-selected patch from each producer, in order:

1. `ansi-export/0001-ansi-export.patch`, immutable ANSI export and common extension information. Maurice owns it.
2. `terminal-profile/0002-terminal-profile.patch`, constructor-only Native/Legacy policy and the sole typed parser State. Cora owns it.
3. `snapshot-budget/0003-snapshot-budget.patch`, atomic profile-aware snapshot encoding and bounded buffer decoding. Brooklyn owns it.

Heather owns composition into the existing source preparation, build and installed-header producer. Do not apply archived checkpoints as additional patches or replace a prerequisite module with an overlay. A composed build is accepted only after the actual three patches apply cleanly and their matching source, headers and binaries are tested together.

The source archive SHA-256 is `333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa`; dependency archive SHA-256 is `91a4e10960a52bf5bf4b15aa2a2cddaff8a89d6cd903d3055bda4fd17de3356d`. The original signed archives and signed checksum manifest are unchanged. Exact Zig **0.17.0** is required. Its isolated Windows x64 archive was checked against `b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428` before use. No compiler is installed globally.

## Common ABI and compatibility identity

Public extension headers are under `ghostty/vt/`, never the internal application-embedding `ghostty.h`.

`ghostty_shellfox_extension_info` uses a sized/versioned `GhosttyShellfoxExtensionInfo`. It reports separate ANSI, terminal-profile and snapshot-budget ABI versions, upstream snapshot-format version, compiled feature bits, declared engine-profile bytes and the named legacy width profile. Absent producer patches report ABI zero and their feature bit remains absent. ABI presence does not mean every terminal profile or state is supported.

The build option `-Dshellfox-engine-profile=<64 lowercase hex>` supplies an externally computed compatibility identity. It is **not authenticity**. Verify the source/artifact manifest and signatures separately.

Compute the final identity from a canonical, versioned manifest containing the original archive hashes, exact Zig version, ordered patch paths/hashes, compiled feature/options and declared compatibility policies. Do not hash a value embedded in its own patch. Do not use a checkpoint's repeated-character test identity as the accepted engine identity. Platform artifact checksums and approved cross-target compatibility evidence remain separate from this declared identity.

An official old DLL does not export these new entry points. A caller requiring full export or a terminal profile must reject an absent symbol, wrong ABI, wrong profile or missing capability. It must not silently fall back to a lossy formatter and label that response full state.

## Immutable ANSI export

The new byte-buffer API is `ghostty_terminal_export_ansi_full_buf`, declared in `ghostty/vt/shellfox_ansi.h`. Original functions and signatures remain unchanged.

The operation is serialized with all source terminal access under the caller's exclusive ownership. It never feeds a source or clone, changes the source viewport/selection, drains a query queue, or invokes source PTY/clipboard callbacks. It reads both screen objects directly inside the pinned library, rather than using destructive VT screen switches on the source. Representation caches may be read or decompressed; this is not a bitwise heap snapshot.

The accepted-state pipeline is:

1. Check exact option size/ABI, source processing status, declared compatibility identity and all target baseline/configuration fields.
2. Preflight unsupported state and source row/cell bounds before producing output. Return reason bits, never a degraded success.
3. Copy a bounded available continuation. Missing tracking for unfinished input is unsupported, not an empty ground suffix.
4. Count into a bounded writer. An output limit failure does not write a caller buffer.
5. Format into a bounded staging allocation, then copy the complete result only on success. Undersized buffers remain untouched and report required capacity. Other failures report zero written bytes.
6. Append the exact continuation last. It may be incomplete UTF-8. This is a byte format, not a drop-in JSON-string v1 recovery payload.

The validated Native cases restore active and inactive buffers/history, each saved/current cursor and pen, G0/G1 invocation, margins/origin, the saved mode bank, effective mouse precedence, nonblank pending wide wrap, dynamic colors/title/PWD and the entire logical eight-entry Kitty keyboard ring when the target declares that capability. Source snapshot bytes before/after export are compared in tests. Native future suffixes exercise DECRC, alternate exit, mode restore, keyboard pops, REP, continuation and resize behavior.

### Fresh-target and reset configuration

`target_is_fresh=1` is required. Construct and configure a fresh passive target with the same grid and declared reset baseline before feeding these bytes. This API is not an in-place reset of an arbitrary dirty terminal.

A measured counterexample explains the restriction: RIS changes the C constructor's `shell_redraws_prompt=false` policy to `true`. A visually identical restore can then behave differently on later OSC 133 plus resize. Native export no longer uses RIS as a supposed universal reset. It starts from the declared fresh baseline and reconstructs the captured shell-redraw policy explicitly. Raw snapshot v1 already has a `shell_redraw` field; that separate decode path still needs its own suffix regression, not an assumption based on this ANSI counterexample.

The caller declares target reset colors/palette, cursor default style/blink, scrollback byte/row policies, runtime image-storage limit and required `runtime_resize_pull_scrollback` as u32 `0/1`. Source values must match exactly. The latter is restoration configuration, not data inferred from the build hash. Both values have resize-suffix tests, and mismatches fail preflight. Unlimited scrollback is `SIZE_MAX`; public getters report `GHOSTTY_NO_VALUE` for unlimited values.

Image replay is outside this ANSI checkpoint: live image/glyph/DnD content is unsupported. An accepted empty Native image state still requires the exact declared runtime storage limit and direct-only image I/O. A zero image/decompression **admission** budget in the snapshot path is different from the future runtime limit and from optional history recompression. No caller callback or filesystem/shared-memory image I/O is restored by ANSI export.

Destination-owned geometry/focus/visibility, presentation caches, selection, viewport and external I/O callbacks are not serialized application authority. Receivers remain passive and must suppress query replies and external effects. Native core modes/encoding state must not be confused with per-view preferences or permissions.

### Explicit unsupported cases

The exporter rejects state outside its validated profile. These include nonmatching reset configuration, arbitrary transition-mode combinations, unsafe OSC metadata, cell hyperlinks/protected or semantic state not handled by this checkpoint, unsupported charsets, hollow cursor states not expressible through the selected sequence policy, blank pending-wrap cases, unavailable continuation, and REP states without a verified reconstruction path. Some are inherently target-limited; others need further implementation and tests. They are not silently stripped.

The Native-only producer checkpoint also rejects every Legacy export with `PROFILE`. This is deliberate until Cora's sole checked State capture is connected and exact Legacy join-state reproduction is proven. A current width match is not sufficient. The ANSI/profile-state bridge has its own prerequisite ABI marker and must agree with the compiled common terminal-profile ABI, or export fails closed.

## Legacy width, cluster and parser state

The shipped renderer uses `@xterm/xterm` **6.0.0**, Terminal plus Fit, without a Unicode addon. Its provider is UnicodeV6, not Ghostty's current width table.

There is one attributed shared module, `src/terminal/shellfox_unicode_v6.zig`, SHA-256 `2167c46bf05f2ecbb35a247f11216d44664b52c47c63bee86947823ddfa1fc90`. ANSI's `legacy_width.zig` is only a thin import. Cora's profile parser consumes the same checked `properties(cp,preceding)?u32`, `width`, `validState` and `clusterWidth` functions. No second generated table or competing serialized State is allowed.

The port preserves the exact shipped interval tables and width-zero joining behavior, with xterm's MIT copyright and complete license retained. The JS reference checks all `0..0x10ffff` codepoints and all five reachable predecessor values `0,2,3,4,5`. Zig computes the same exhaustive SHA-256 vectors. Surrogate slots are included in the table-function fingerprint; that is not permission to accept invalid UTF-8/scalars in a stream.

Constructor profiles Native `0` and Legacy `1` are distinct from snapshot wire-profile IDs. The old constructor stays Native. Profile State is Cora's single sized/versioned `GhosttyShellfoxTerminalProfileState`, containing terminal profile and exact preceding-join state. Native requires zero legacy state. There is no public setter that silently migrates an existing terminal.

Capture State and body at one consumed-input cut. Restore the explicitly captured State on fresh decoded data before continuation replay, and compare post-replay State and subsequent live suffixes. Do not infer/reset join from cells, constructor defaults, build hashes or arbitrary partial ESC bytes. Controls and REP have shipped-parser semantics that must be demonstrated through differential tests.

Legacy live/export acceptance remains blocked on the standalone width-zero representation and explicit layout2/codec work recorded by Cora. Valid shipped-xterm behavior cannot remain a final generic live-input rejection. Newly allocated layout2 bits must have named semantics and a distinct codec/profile/version; raw-v1 readers must reject them, not reinterpret old reserved data.

## Snapshot budgets

Brooklyn owns the atomic buffer encoder companion and `ghostty_shellfox_snapshot_budget_decode_buf`, with its separate header/module. Original unbounded decoder entry points remain ABI-compatible but are not the safe path for untrusted new wrappers.

Admission limits must be enforced before READY and native page allocation, including pooled backing and direct mmap/VirtualAlloc. A custom heap allocator alone is not sufficient. Require bounded input/work/rows/cells/pages/history/continuation/metadata/image handling, checked profile/state, FINISH and exact EOF before returning an accepted terminal. Wire decompression and history recompression are separate policies. Missing Legacy State rejects. This ANSI producer does not claim those decoder protections were implemented or verified by its tests.

## Producer evidence

All evidence is a disposable Windows x86_64 GNU producer checkpoint, not host/desktop/mobile or cross-platform acceptance. The original staged mobile binaries are untouched.

- Exact isolated Zig 0.17.0 archive hash/version verified.
- Offline `--system` dependencies and baseline CPU used.
- ReleaseSafe selected native C-library test binary: 52 tests passed, including named extension tests plus unnamed upstream declaration tests. This is not all upstream tests.
- Exhaustive JS and Zig width/join reference vectors passed.
- Shared/static library compilation and strict C11 header/import-library/DLL smoke passed at the recorded checkpoint. Every ABI update requires a matching rerun; older sizes are historical evidence only.
- An unfiltered ReleaseSafe upstream-suite attempt hit an existing debug-only `pinIsValid` comptime assertion. A Debug unfiltered compile exceeded its 360-second limit. Neither is a pass or a suppressed test result.

Reproduce a prepared patched source checkpoint with the new `tools/test-native.mjs`, supplying source, offline deps, exact isolated Zig and an external test profile. Its work/cache/output stays under repository-root `tmp/` and is removed unless `--keep` is explicit. No visible GUI, user-account traffic, release action or global tool change is involved.

Each new Zig module is below 350 lines. The generated unified patch is larger because it is one atomic, compilable source delta. Existing cohesive upstream Config/Terminal modules are not split merely to reduce patch-file line counts. Preserve the original MIT notices, the retained xterm license and generated-table provenance in source and distribution notices.

Before application use, compose all three exact patches, regenerate the profile from ordered hashes/options, validate the final public headers and ABI sizes, and build matching host/desktop/mobile artifacts. Do not relabel old official archives, transient checkpoints, or historical header proofs as that acceptance build.
