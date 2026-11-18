# ANSI/common extension producer checkpoints

Patch order is ANSI/common **1**, Cora terminal-profile **2**, Brooklyn snapshot-budget **3**. Apply one manifest-selected revision from each producer, not every `.patch` file in this directory.

## Current next candidate

`checkpoints/native-002/checkpoint.json` selects the revised Native-only patch. It is not a final accepted engine, header set or artifact. Its patch SHA-256 is `68d3e2d557594ae91bf4655c6a4fc9bde5a89cbe3958053cb61b0e09cd00183d`.

The top-level `0001-ansi-export.patch` and `checkpoint.json` retain the historical first checkpoint SHA `9844b8c9fd007347cb2bb55962e316cf4f50194b6e5e9fd8858a2afe706cfdd5`, because existing profile-producer evidence names it. The same first checkpoint is archived under `checkpoints/native-001/`. It has the subsequently detected RIS/shell-redraw gap. Do not treat it as the revised code or silently replace its evidence.

Revision 2 requires an explicitly fresh passive target and checked declared reset/runtime configuration. It restores saved cursors, inactive buffers, saved modes, charsets, effective mouse state and complete logical Kitty stacks for the validated Native cases. Unsupported states fail before publishing bytes. Legacy exports are explicitly unavailable until the sole checked Cora State bridge and Legacy layout/continuation behavior pass acceptance. This is not universal FullState.

Public headers are `ghostty/vt/shellfox_extension.h` and `ghostty/vt/shellfox_ansi.h`. The latter declares `ghostty_terminal_export_ansi_full_buf`. Original function signatures remain unchanged. No internal `ghostty.h` is used. `runtime_resize_pull_scrollback` is required u32 `0/1`, not a guessed constructor default. Current x64 C smoke sizes are extension info **72**, ANSI options **144**; these do not replace historical or future snapshot-budget ABI proofs.

Common extension information has separate `ansi_abi_version`, `terminal_profile_abi_version` and `snapshot_budget_abi_version`. Absent patches report zero for their ABI and no feature bit. The declared `-Dshellfox-engine-profile` is not artifact authentication. Final identity must be computed externally from the signed archive hashes, exact Zig version, ordered patch hashes and options, without a self-referential embedded patch digest.

## Profile-state bridge for patch 2

Revision 2 adds `src/terminal/shellfox_ansi/profile_state.zig` so patch 1 compiles independently. Its marker is zero and it only admits original Native terminals. Preflight rejects a mismatch between this marker and the common terminal-profile ABI.

Cora's patch 2 must replace that small bridge with her one checked capture, not a second State or table:

```zig
const T = @import("types.zig");
const profiles = @import("../shellfox_terminal_profile.zig");
pub const terminal_profile_abi_version: u32 = 1;
pub fn check(t: *const T.Terminal, options: *const T.Options) u64 {
    const state = profiles.capture(t) catch return T.Reason.profile;
    return if (state.terminal_profile == options.target) 0 else T.Reason.profile;
}
```

The separate Legacy export gate stays closed until actual preceding-join state reproduction is implemented and tested. Missing or invalid Legacy state cannot be replaced by zero or inferred from widths. Patch 2's prerequisite manifest needs an explicit rebase to the selected revision; applying an old checkpoint and calling that a new composition is not evidence.

## Shared Unicode table

`src/terminal/shellfox_unicode_v6.zig` remains exactly SHA `2167c46bf05f2ecbb35a247f11216d44664b52c47c63bee86947823ddfa1fc90`. ANSI's `legacy_width.zig` is a thin import. Cora's profile consumes its checked `properties(cp,preceding)?u32`, `validState`, `width` and `clusterWidth`; do not overlay another generated table or `charProperties` definition.

It is ported from the shipped `@xterm/xterm` 6.0.0 UnicodeV6 provider, not a Unicode addon. `XTERM-LICENSE`, the source hashes and exhaustive reference vectors are retained. The patch also retains the license beside the generated upstream module. Distributions must ship this notice with the original Ghostty/dependency notices.

## Repeatable checks

From the repository root:

```text
node crates/ghostty-vt-sys/build-support/patches/ansi-export/tools/verify-width.mjs
node crates/ghostty-vt-sys/build-support/patches/ansi-export/tools/test-native.mjs \
  --source PATCHED_SOURCE --deps OFFLINE_DEPS --zig ISOLATED_ZIG_0_17_0 \
  --profile EXTERNAL_TEST_PROFILE
```

The second command uses only project-root `tmp/`, checks exact Zig 0.17.0, builds the selected ReleaseSafe native C tests and shared/static library, and compiles/runs a strict public-header C11 import-library/DLL smoke. No GUI, PTY, user data, account traffic, release or global tool installation is involved. `--keep` explicitly retains disposable output; otherwise it is removed.

The reference check covers all 1,114,112 codepoint slots and all five reachable preceding-property values. Invalid scalar/UTF-8 admission remains a separate parser responsibility. `tools/generate-width.mjs` refuses an unpinned source hash; run exact Zig 0.17.0 formatting on regenerated output before comparing the authoritative module checksum.

Revision 2 evidence is 52 selected native tests, a matching shared/static build, strict C11 header/DLL smoke and exhaustive JS/Zig reference vectors, all on Windows x86_64 GNU. This is not all upstream tests or host/desktop/mobile acceptance. An unfiltered ReleaseSafe suite hits an upstream debug-only comptime assertion; a Debug unfiltered compile exceeded 360 seconds. Neither passed.

See `docs/ghostty-compat-extension.md` for accepted-state constraints, reset/resize policy, pending continuation and passive effect handling. New code modules are below 350 lines. Unified patches and edits to existing cohesive upstream files are larger because an atomic, compilable patch must not be split arbitrarily.
