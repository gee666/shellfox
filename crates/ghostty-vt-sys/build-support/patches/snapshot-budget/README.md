# Bounded snapshot codecs

This is an additive, Native-only producer candidate, not final engine/application
acceptance or universal FullState. Legacy and unsupported state fail closed.
No original C function signature, signed archive, staged mobile library,
application consumer wiring or release was changed.

## Exact candidate and patch order

Use `checkpoints/native-002/checkpoint.json`, not the loose top-level review draft.
The candidate was published as one directory after testing these exact inputs:

| Order | Candidate | SHA-256 |
| --- | --- | --- |
| 1 | `../ansi-export/checkpoints/native-002/0001-ansi-export.patch` | `68d3e2d557594ae91bf4655c6a4fc9bde5a89cbe3958053cb61b0e09cd00183d` |
| 2 | `../terminal-profile/checkpoints/native-002/0002-terminal-profile.patch` | `8cc4f2403f4fe69faf1b77916d2db8ac3f713824dbfe273ba714c0a3fdf02d22` |
| 3 | `checkpoints/native-002/0003-snapshot-budget.patch` | `28fd8c68edebdd8a014c40123261a2494ea3bde47089b3c2e4dd3cf474012a00` |

Apply with `patch --batch --fuzz=0 -p1`. Offsets are allowed; fuzzy matching is
not. Patch 2 installs the actual ABI1 ANSI profile-state bridge, using Cora's
single checked capture routine. Patch 3 changes only the common
`snapshot_budget_abi_version` declaration from 0 to 1. Common feature bits
come from compiled ABI availability. Unknown/mismatched declarations reject.

The shared attributed width/join module remains
`src/terminal/shellfox_unicode_v6.zig`, SHA-256
`2167c46bf05f2ecbb35a247f11216d44664b52c47c63bee86947823ddfa1fc90`.
This patch introduces neither another table nor another profile State.

The engine identity is supplied externally from ordered patch hashes and build
options. `checkpoints/native-002/engine-profile-input.txt` is the tested preimage;
its SHA-256 is `939d2a42b92dab06a5d9879bf75ecfc44918efcb27ea40e678038e00f55f33b2`.
It is compatibility metadata, not artifact authentication. No self-referential
patch hash is embedded in source. Verify source/artifact manifests separately.
Do not make this test identity a final accepted engine profile.

## Public API

Include `ghostty/vt/shellfox_snapshot_budget.h`, not the internal application
embedding header. Common info and terminal-profile headers stay separate.

```c
GhosttyResult ghostty_shellfox_snapshot_budget_decode_buf(
    const uint8_t *bytes, size_t length,
    const GhosttySnapshotBudgetOptions *options,
    GhosttyTerminal *out, GhosttySnapshotBudgetReport *report);

GhosttyResult ghostty_shellfox_snapshot_budget_encode_buf(
    GhosttyTerminal source, const GhosttySnapshotBudgetEncodeOptions *options,
    uint8_t *output, size_t capacity, size_t *out_written,
    GhosttyShellfoxTerminalProfileState *out_state,
    GhosttySnapshotBudgetReport *report);
```

The exact full structs are in the patched public header. Every public options,
report and State has exact size/ABI-version checks. Unknown ABI, wire profile,
format/library version, build identity, flags or unsupported policy fails rather
than silently selecting another format. State is Cora's canonical ABI1
`{size, abi_version, terminal_profile, preceding_join}`. Never serialize padding.

Windows x64 C/DLL proof verifies decode-options size 184, encode-options size
168, State size 24 and report size 48. The new resize-field offsets are 148/132.
Equal sizes do NOT make earlier transient headers compatible; padding absorbed
new fields. Use exact header and ordered-build digests. The current budget
header SHA-256 is `fa35639f1759a7804f6953cc68f467b3e664d33352bd8b6a5a756deb71a8a3a3`.

Wire profile 1 is this defined raw-v1 restoration profile. Terminal profile 0 is
Native. Terminal profile 1 is LegacyXterm6, currently explicitly unsupported.
Raw bytes/build identity alone do not prove per-terminal parser State. Decode
requires captured State and exact restoration configuration from the envelope.
Restore runs before continuation replay; capture after replay must equal the
source cut. No published terminal is switched to another parser profile.

## Atomic ownership and publication

The caller exclusively owns the source for the encoder's whole call. State,
continuation and body are captured internally at that cut, not through a racy
getter followed by an unprofiled encoder. Source is not fed, switched, queried
for PTY replies, or mutated. No source effects or custom allocator callback is
invoked. The body is staged privately under the budget.

Encoder failure leaves caller bytes untouched, sets `out_written=0`, and
invalidates a valid output State with size/ABI zero. Reinitialize its header
before retry. Outputs must be disjoint and not alias source-owned storage.
Success publishes complete body and canonical State. Send them with the exact
validated wire/config metadata. Encoder report source offset is zero.

Decoder failure clears `out`. No READY handle escapes. Success requires FINISH,
exact EOF, exact history metadata agreement, no dropped history pages, valid
profile State and supported configuration. Result ownership uses the original
`ghostty_terminal_free`. Returned vtables retain no stack-scope pointer.

Input/options/identity bytes must remain immutable during the call. The budget
ends on return, not on later writes/resizes. Application sequence-barrier commit,
passive mirror policy and live-input budgets remain application responsibilities.
Never use the original unbounded decoder on untrusted recovery input.

## Defined restoration configuration

See [runtime-config.md](runtime-config.md) for the audited fields and limitations.

Required explicit configuration includes exact per-screen
`runtime_image_storage_limit`, direct-only image I/O, and
`runtime_resize_pull_scrollback` u32 0 or 1. Encoder checks actual source values;
decoder applies the resize flag before continuation. False supports ConPTY.
No zero or guessed live image/default resize policy is substituted.

Shell redraw is already encoded at TERMINAL byte 32. Tests preserve the C false
policy and true after RIS, crossed with resize-pull false/true, then compare an
OSC133/resize suffix. This does not fix or waive the separate ANSI RIS gap.

Other unencoded C-stream settings must match the defined canonical profile.
A pure preflight rejects custom terminfo/APC/protocol/report/unknown/clipboard
limits, checksum variants, glyph registrations, drag/drop, clipboard sessions
or grants, and semantic failures. Source callbacks are deliberately not restored
to the passive destination. A pending APC builder is unsupported because its
historical builder configuration is absent from raw-v1.

## Wire and resource limits

PAGE records are raw, not compressed wire pages. The distinct decoder
`compress_history` option recompresses restored history; this profile disables
it. Image/decompression admission counts/bytes stay zero. Nonempty images,
placements/loading, unknown records and compressed source pages are unsupported.
Empty Kitty storage markers preserve both ID counters. A new internal encode
option emits them only for the companion; its default is false, preserving
original Native public encoder bytes. Cora owns the separate original-encoder
Legacy guard in `c/snapshot.zig`; this patch does not modify it.

Every budget is finite; zero is zero, not unlimited. The allocation-free admission
pass checks source size, records/pages, dimensions including pixels, cells,
history and continuation before native allocations. Additional profile bounds
keep native arithmetic/refcounts safe: terminal dimensions/page width at most
4096, at most 65,000 cells per page, and string/grapheme capacity hints at most
64 MiB each. Page height can reach u16 range when width is small.

A `GhosttyAllocator` alone is NOT a hard budget. PageList pools use a separate OS
allocator; direct Pages use mmap/VirtualAlloc. The patch accounts for both plus
the codec heap. Heap allocations use OS pages rather than malloc, so object
metadata is not hidden outside accounting. Charge precedes allocation, including
pool preheat, growth, remaps, continuation and temporary staging. Windows uses
64 KiB reservation granularity; other native systems use runtime page size.
Over-page alignment precharges transient/over-aligned reservations. Frees do not
refund cumulative charges. Refused or backend-failed optional allocations poison
the transaction even if upstream recovers; no degraded result is published.

This bounds codec-owned cumulative reservations, NOT process RSS. Caller input,
stacks, library code, page tables and other threads are excluded. Thread-local
synchronous scopes isolate calls; same-thread nested entry is unsupported.

Work uses deterministic conservative admission counters, not a timer race or OS
CPU quota. It charges record bytes/framing, grid/table/string comparisons,
continuation validation/replay, rounded allocations and profile cell validation.
Checked arithmetic rejects overflow. See source for the exact formula. Builds
with slow integrity verification and unsupported mapping targets return
`GHOSTTY_UNSUPPORTED`; they are not advertised as safe by pretending a custom
heap bounds all memory. Freestanding/Wasm are outside this profile.

## Verified Native002 evidence

The candidate uses authenticated upstream source/dependency pins, unchanged:

- Source `333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa`.
- Dependencies `91a4e10960a52bf5bf4b15aa2a2cddaff8a89d6cd903d3055bda4fd17de3356d`.
- Windows Zig 0.17.0 archive
  `b5663f69581dcf391293fbf16c06cb80d81d806545ce618b4d0bab7f0eb8c428`.

Actual revised 1→2→3, Windows x86_64 GNU, ReleaseSafe, baseline CPU, default
features, app version `1.4.1-dev`, library `1.0.1`:

- Combined `snapshot`/`Shellfox` filters: 452 passed, 1 intentional skip.
- Budget filter: 92 passed, 1 intentional skip, including 15 budget-named tests.
- Strict C11 public-only consumer linked to the actual GNU DLL/import library:
  passed both APIs, ABI/config/identity checks, live suffix, copy/cursor/State,
  no partial encoder publication, budget failures and Legacy refusal.
- Independent fresh extraction applying the three exact patches with fuzz0
  matches built source, excluding build cache only.

The skip is the unavailable-build refusal test on a supported build. Current
budget code also passed snapshot-disabled tests on the earlier prerequisite
composition, and Linux x64 musl cross-compilation, not native runtime. These
are not relabeled as Native002 whole-source/platform execution.

Native differentials cover partial UTF-8/CSI/OSC, controls/REP, both screens,
copy/cursor/style/State, and the shell-redraw/resize matrix. Negative tests cover
truncation, CRC/FINISH/trailing bytes, unknown versions/records/profiles,
preallocation caps, hostile history policies, oversized hints/counts, partial
failure cleanup, mapping budgets, over-alignment, recovered OOM and callbacks.

The earlier 9844/e392/6ccd/424-test/C proof is historical. Its source, DLL and logs
remain under project `tmp/ghostty-budget/historical/native-6ccd`; its old header
proof is not reused for the required new resize field. `pnpm typecheck` passed
once during this task; no TypeScript application code changed.

All upstream tests, other-OS native execution, app delivery and Legacy parity are
not claimed. The patch container exceeds 350 lines because it contains several
focused modules; every new module stays below 350. Narrow hooks remain in the
existing cohesive upstream PageList/page/C-terminal/snapshot files.
