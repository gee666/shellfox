# Bounded format2 proposal

Historical approved design and coordination record. Implementation now consumes
actual corrected P4-002 and is published at checkpoints/experimental-001. See
README.md and that checkpoint for current code/proof, exact payload behavior and
remaining gates. Earlier future-tense blockers below record the design phase,
not a claim that current source is missing. Normal Legacy availability stays OFF.
Native002 files, original Native defaults and their recorded proofs are immutable.
Cora owns the core cell/layout contract. This codec will consume that contract;
it will not add another parser, width table, join State, or guessed layout state.

## Requires and patch order

Apply to the authenticated vt-1.0.1 source/dependency archives, then:

1. ANSI Native002, SHA-256
   `68d3e2d557594ae91bf4655c6a4fc9bde5a89cbe3958053cb61b0e09cd00183d`.
2. Terminal-profile Native002, SHA-256
   `8cc4f2403f4fe69faf1b77916d2db8ac3f713824dbfe273ba714c0a3fdf02d22`.
3. Snapshot-budget Native002, SHA-256
   `28fd8c68edebdd8a014c40123261a2494ea3bde47089b3c2e4dd3cf474012a00`.
4. Cora's NEW Native002-rebased canonical layout/capture/restore contract.
   Its exact published patch/hash is required before cell codec implementation
   or a composable producer candidate.
5. This NEW bounded format2 delta, with its own versioned candidate and digest.

The historical `88f5...` layout prototype is NOT this prerequisite. Its recorded
alternate hidden-column failure cannot be masked or treated as accepted state.
No existing Native002 checkpoint or top-level historical proof is relabeled.
`requires.json` remains blocked until Cora publishes an exact compilable contract.

## Public names and availability

Proposed separate header `ghostty/vt/shellfox_snapshot_format2.h` and C module
`src/terminal/c/shellfox_snapshot_format2.zig`:

```c
GhosttyResult ghostty_shellfox_snapshot_format2_encode_buf(...);
GhosttyResult ghostty_shellfox_snapshot_format2_decode_buf(...);
```

Exact sized/versioned options and output types wait for the canonical layout
contract. They reuse the sole `GhosttyShellfoxTerminalProfileState` ABI, not a
second width/join wire struct. The codec supplies explicit declared runtime
configuration and exact engine identity, never guessed source defaults.

Public Legacy construction/negotiation/export/restore availability remains OFF.
Core-only prototype tests may exercise a Cora-owned fresh restore hook under
`builtin.is_test` or an explicit separate `-Dshellfox-experimental-format2` test
build gate. Normal/public Legacy remains OFF. No live-terminal setter,
matched-wrapper claim or staged artifact follows from this prototype.

## Distinct framing, checked before cells

Proposed fixed prefix, unsigned little-endian fields:

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 8 | Magic bytes `53 46 4f 58 53 4e 32 00`, `SFOXSN2\0` |
| 8 | 2 | Format version 2 |
| 10 | 2 | Prefix size 32 |
| 12 | 4 | Wire profile 2, distinct from raw-v1 profile 1 |
| 16 | 4 | Required Cora layout-contract ABI, exact published value |
| 20 | 4 | Flags, zero in the first profile |
| 24 | 8 | Exact complete encoded length including FINISH |

The decoder verifies magic/version/profile/ABI/length before any cell decoding
or native allocation. It never sends layout2 cells through the raw-v1 decoder,
whose invalid-codepoint normalization must remain unchanged. No invalid scalar
sentinel or formerly reserved raw-v1 bit identifies format2. Raw1 APIs and bytes
retain their existing meaning; no fallback or implicit reinterpretation occurs.

Record framing may reuse the existing CRC32C implementation and 10-byte
`u16 tag/u32 payload length/u32 CRC` encoding, but not its unknown-tag skipping.
Format2 uses a new strict tag family and requires exact payload exhaustion.

Proposed required ordering:

1. `0x2001 COMPATIBILITY2`: exact engine identity, format/layout/profile ABIs,
   captured canonical profile State fields and declared runtime configuration.
2. `0x2002 TERMINAL2`: complete supported terminal-wide metadata, with explicit
   source semantics. Any unencoded state is explicit configuration or rejected.
3. `0x2003 SCREEN_LAYOUT2`: one manifest for each declared screen, before pages.
4. `0x2004 PAGE_LAYOUT2`: physical geometry and resolved per-row backing lengths.
5. `0x2005 PAGE_CELLS2`: strict canonical cells/tables/text for that exact page.
   Repeat page groups in declared screen/order, including inactive/history data.
6. `0x2006 CONTINUATION2`: exact replay-safe bytes captured at the source cut.
7. `0x2007 FINISH2`: record/page/physical-slot/text-scalar totals, all u64, matching
   the measured stream. Exact EOF follows; no trailing bytes are accepted.

There is no published READY terminal. All manifests and cell bytes are first
admitted without native allocation. Unknown tags/flags/versions fail closed.
COMPATIBILITY2 embeds the sole canonical State fields and explicit runtime
configuration INSIDE the same atomic blob. It never serializes C size_t,
padding or pointers. Decode options supply expected engine/profile/layout ABI
and finite bounds; embedded typed fields validate before fresh restore/replay.
Detailed payload packing is not frozen until Cora's canonical types are known.

## Canonical core contract needed

Cora must publish the actual source functions/types and an exact prerequisite
hash, including read-only capture and checked fresh restore with these meanings:

- visible geometry versus owned physical/backing geometry, on both screens;
- explicit per-row backing lengths, including ragged alternate rows after a
  width shrink, row growth and later intermediate-width grow;
- every owned physical slot, including hidden suffixes and padding/unused stride
  storage. If padding is required empty, core validation must enforce that;
- canonical cells, scalar/grapheme/style/hyperlink ownership and legal tags;
- semantic width separate from physical advancement. Standalone-zero and
  orphan-wide physical-one cases must not be guessed from visible width;
- current and saved virtual cursor/pending/overflow fields and their bounds;
- the existing canonical width/join State, kept separate from layout state;
- supported terminal/screen metadata, history, modes/stacks and future behavior;
- staging/validation before continuation replay, followed by checked capture.

The interface must not force the codec to copy a stale visible-only screen,
feed/query/switch the source, infer row lengths from text, or restore unknown
state as zero. Captures requiring allocations must accept the charged allocator.
Any unmetered page/hidden/text allocation or work path blocks safe support.

## Coordination against Cora's published contract document

The namespace in `terminal-profile/prototype/FORMAT2-CONTRACT.md` is the intended
single source: `screenLayout`, `rowBackingCols`, `semanticWidth`, `fromRaw`,
`physicalAdvance`, `validateLayout` and the checked pre-continuation restorers.
The document is not itself a compilable prerequisite; its source/hash is pending.

Its explicit override values are 0 native, 1 semantic-zero, 2 orphan-wide;
3 is invalid. Override1 may contain standalone text or an empty orphan stub.
Override2 is semantic width2 with physical advancement1 and no structural tail.
Both current/saved cursors carry physical X, Y, pending-wrap and overflow.
These are separate from the existing width/join State.

Before implementation the producer must settle two concrete API gaps: an exact
read-only native-wide/semantic-override capture getter, and fresh profile staging
so layout validation cannot accidentally treat Legacy cells as Native. Width and
physical advancement alone do not uniquely recover all override representations.
The codec will not inspect unpublished flag bits or set profile fields directly.
The sanctioned internal test restore hook must keep public Legacy unavailable.

## Cell representation

New records contain explicit pointer-free canonical cell values, not copied
native packed words or reserved-bit tricks. Unicode scalars are validated before
native enum/cell construction. Physical slot occupancy, semantic width and text
ownership are separate meanings supplied/validated by Cora. Styles, hyperlinks
and grapheme sequences use bounded references and lengths. No second Unicode
provider or VT parser implements width/join behavior in this codec.

Per-row lengths alone are not an allocation bound. A strided page whose visible
width is 4 and physical stride 8 is charged for all 8 slots per stored row, even
where a ragged row's semantic backing length is only 4. All retained hidden cells
and managed text are captured, or the core must certify empty unowned padding.

## Resource admission

Reuse Native002's scoped allocation accounting where valid, without modifying
that immutable patch. Decoder/source-capture allocations must use the charged
heap, pooled-page and direct-mapping hooks. Cora's new hidden/backing allocation
paths must participate too. The limit is cumulative OS-rounded reservations,
not malloc requests alone or process RSS. No refunds or partial-result success
are allowed after optional-allocation failure.

New explicit count limits cover screens, all physical pages/rows/slots, per-row
and total backing storage, visible dimensions, history, text scalars/grapheme
bytes, style/link tables and strings, continuation and encoded bytes. Visible
cells must never substitute for physical storage. Native refcount/offset limits
still apply; checked arithmetic precedes every size conversion and allocation.

Work uses deterministic counters for record bytes, every physical slot, row
metadata, text/codepoints, table operations, validation and replay. Cora helpers
must expose bounded work or accept a charging mechanism. Unsupported work or
compression paths return unsupported rather than pretending a wall-clock race
or custom heap covers them. Image/decompression admission remains zero initially.

## Atomic encode and staged decode

Encoder holds the caller's exclusive source ownership throughout canonical
State/layout/config/continuation capture, admission, body generation and
validation. It stages privately. Failure leaves caller bytes untouched, publishes
no length/artifact and invalidates output metadata. No callback or source feed,
query, screen switch or pending-reply drain is permitted.

Decoder validates the whole envelope, records, manifests, references and budgets
before native allocations. A core factory creates a fresh isolated terminal in
the exact declared profile/layout, never by first normalizing cells as Native
raw-v1. Populate physical pages/rows/cells and metadata through Cora's checked
restore contract. Install captured layout, virtual cursor and width/join State
BEFORE replaying continuation. Validate canonical post-replay capture and exact
FINISH/EOF before publication. Every partial failure frees all owned allocations.

## Current framing proof

`overlay/` contains only the independent new framing modules and standalone test
root. Verified local Zig 0.17.0, ReleaseSafe, actual frozen Native002 source base:
11/11 selected framing/resource tests passed. They cover the approved prefix, reciprocal
raw1 magic rejection, all prefix/record truncations, version/layout/profile/flag
and exact-length refusal, CRC/tag/order/FINISH/trailing errors, measured-total
comparison, deterministic work and record/page/screen limits without allocation.
Resource tests additionally charge hidden physical stride and ragged-row padding,
text scalars/bytes, overflow and work refusal before any allocation.
Opaque test payloads are explicitly NOT a substitute canonical layout or cell
codec. No cell/body/C-ABI success or public availability is claimed by these tests.

## Required proof before any availability claim

- Both decoders reject the other magic before cells; malformed layout2 scalars
  and tags cannot reach raw1 normalization or native enum instantiation.
- All truncations, CRC/length/totals/row/reference/cursor/profile errors fail
  without output replacement, callbacks, OOM abort or accepted trailing data.
- Physical-hidden amplification hits slot/page/text/work/allocation caps before
  native allocation, including ragged rows and unowned-stride padding.
- Every allocation-failure boundary unwinds; encoder publishes no partial pair.
- Source → format2 → bounded decode → live suffix compares canonical State,
  visible cells/cursor, full backing/copy/history and both saved cursors.
- Include zero/orphan-wide, shrink/grow, ICH/DCH/ECH/EL/ED, scroll/new rows, reflow,
  both screens and partial UTF8/CSI/OSC/controls/REP at actual source cuts.
- Preserve original Native defaults and byte-identical raw-v1 fixtures against
  frozen Native002; no Legacy layout is smuggled into old records or bits.
- Compile actual ordered producer sources with verified local Zig0.17.0 and
  strict public C-header/DLL proof. Matched wrappers/artifacts, rendering/edit/
  reflow/replay and supported-platform execution remain separate acceptance gates.
