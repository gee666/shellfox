# Terminal profile producer checkpoint

This is patch **2**. The top-level `0002-terminal-profile.patch` and `checkpoint.json` are the **historical e392 checkpoint**, after historical ANSI1 `9844...`; its old 89-test/C smoke proof is not relabeled.

The revised candidate is `checkpoints/native-002/0002-terminal-profile.patch`, SHA-256 `8cc4f2403f4fe69faf1b77916d2db8ac3f713824dbfe273ba714c0a3fdf02d22`. It requires ANSI `checkpoints/native-002/0001-ansi-export.patch` SHA-256 `68d3e2d557594ae91bf4655c6a4fc9bde5a89cbe3958053cb61b0e09cd00183d`, not top-level historical ANSI1. This candidate adds the ABI1 checked-capture bridge replacement. Actual revised 1→2→budget review draft `721db1...` passed 111 selected tests; exact inputs and scope are in its separate checkpoint JSON. Brooklyn owns publication of the matching revised patch3 candidate.

Brooklyn's bounded snapshot patch is **3**. This checkpoint is not a final accepted engine digest, header set, artifact or FullState claim. Do not wire its test identity into packaging, mobile bindings or the existing prepare producer.

## Supported checkpoint

Native profile `0` is compiled and exercised. The old constructor remains Native with unchanged defaults. The new immutable constructor and read-only getters are exported. The common `terminal_profile_abi_version` changes from `0` to `1`, enabling feature bit `4` only alongside the compiled API. `snapshot_budget_abi_version` remains `0` until patch 3.

Legacy profile `1` is a declared ID, but its public constructor and restoration remain explicitly unsupported while the standalone-zero layout2/render/codec prototype is unfinished. ABI presence is not Legacy availability. Ordinary prototype base/combining/ZWJ/VS16/REP paths are tested internally; that is not live Legacy acceptance.

The complete consumable patch is `0002-terminal-profile.patch`. `overlay/` retains its added source files for review. `upstream.patch` is the same existing-file delta for older local review scripts, not a second patch to apply. `tools/apply.mjs` checks the authoritative shared Unicode prerequisite and applies complete patch2 with fuzz0. It does not replace or generate that table.

## Shared contracts

The sole table is ANSI1's `src/terminal/shellfox_unicode_v6.zig`, SHA-256 `2167c46bf05f2ecbb35a247f11216d44664b52c47c63bee86947823ddfa1fc90`. Its API is `width`, `validState`, checked `properties(cp, preceding) ?u32`, and `clusterWidth`. Parser profiles and ANSI checks consume that module. MIT attribution and source provenance remain with it; this directory also retains the shipped xterm license for its differential fixtures.

The sole public state type is `GhosttyShellfoxTerminalProfileState` in `include/ghostty/vt/shellfox_terminal_profile.h`, mirrored by the Zig extern `shellfox_terminal_profile.State`:

```c
size_t size;
uint32_t abi_version;
uint32_t terminal_profile;
uint32_t preceding_join;
```

ABI1 requires exact size. Native requires zero legacy state. Legacy legal raw xterm property values are `0,2,3,4,5`. There is no competing serialized width/join struct and no public existing-terminal setter.

```zig
restoreStateBeforeContinuation(
    *Terminal, requested: u32, state: ?*const State, max_cells: usize,
) error{UnsupportedProfile, IncompatibleState, LimitExceeded}!void
```

Apply only to fresh decoded state before continuation replay. `applyRestored(*Terminal, requested, max_cells)` validates already-installed state. Missing Legacy state rejects; cells, constructor defaults and build hashes are not state sources. Native restoration does not guess/change runtime image storage policy. Brooklyn supplies the exact declared post-decode limit and direct-only I/O policy separately from image admission budgets.

Capture State at the exact **consumed input cut** paired with the snapshot body. Pending continuation is separate parser/UTF-8 state. Source, post-replay and subsequent live suffix must be compared, including partial UTF-8, CSI/OSC, controls and REP. Do not assume a feed boundary or arbitrary partial escape resets join. The old public snapshot encoders reject Legacy rather than dropping its extra state. Brooklyn owns the profile-aware atomic State/body encoder companion and bounded decode; a separate getter plus an unprofiled encoder is not an atomic artifact.

## Evidence and limits

- Verified original signed source/deps archives and official Windows baseline DLL ZIP; originals unchanged.
- Verified project Zig 0.17.0 archive pin, exact compiler version.
- Fresh exact patch1 then patch2 composition, fuzz0.
- Windows x86_64 GNU, ReleaseSafe, baseline CPU: `test-lib-vt -Dtest-filter=Shellfox` passed **89 tests**, 33 core and 56 C.
- Native old/new constructor snapshot bytes are identical for the tested Unicode plus partial-CSI input under upstream raw-v1. Native State checks and cell budgets are tested.
- All codepoints `0..0x10ffff` and all reachable preceding states match the shipped xterm6 oracle digest `817ebb04d065be74f5c9c9f86940a9bda10931d27803bd1722e0415c5f1abeb3` using the shared module.
- Public C header type/layout compilation and independent verified-DLL counterexample probe are retained in `tests/`.
- This is not all upstream tests, cross-platform runtime acceptance, Legacy snapshot acceptance, pixel/raster acceptance, or complete terminal parity.
- A later measured Native future-behavior gap remains outside this historical proof: RIS changes the C-constructor `shell_redraws_prompt=false` policy to true, and OSC133/resize suffix behavior can differ after snapshot decode if that current policy is unencoded. Brooklyn/Maurice are auditing explicit restoration configuration. The width/join State ABI is not expanded to guess that policy.

`CELL-COMPATIBILITY.md` and `tests/counterexamples.json` record the measured copy/cursor/column gap and proposed explicit new cell-layout/version boundary. No reserved-bit/raw-v1 reinterpretation is accepted. New source files stay below 350 lines; the unified patch and edits to cohesive upstream Terminal/stream modules are not split into artificial files.
