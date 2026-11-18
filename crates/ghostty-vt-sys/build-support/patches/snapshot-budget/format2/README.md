# Experimental bounded format2 codec

Implemented against the actual corrected Cora P4-002 canonical source, not a
layout stub or inferred private memory shape. Native002 remains immutable.
**Normal format2/Legacy availability is OFF.** This is a tested producer
prototype, not app/migration, all-platform or universal FullState acceptance.

## Exact candidate

`experimental-001` is immutable historical evidence. Reagan requested changes;
its hashes/tests are NOT bounded-codec acceptance. The working fixes are published
as a NEW candidate awaiting independent review.

Use `checkpoints/experimental-002/checkpoint.json` and its exact requires, in
order. The new fifth patch is:

`0005-bounded-format2.patch`

SHA-256 `6572bb5ae16175380ed596b670e15c88347dbe130da8f9c668fcae43c109c340`.

Prerequisites are Native002 ANSI `68d3...183d`, profile `8cc4...2d22`, budget
`28fd...2a00`, then corrected P4-002
`f387980a974eb3f65d4c8b85f64d5a31bd6acea42a544ec6c3426b9523e10ac3`.
P4's 25 source-file hashes were checked before codec edits. Its source digest is
`8d203cb2f34e15b58837c10cd6eccc775dd3717c39033fc116aa62ee9eda8833`.
The superseded b011/historical layout prototypes are not this prerequisite.

Apply with `patch --batch --fuzz=0 -p1`. Independent fresh source application of
all five patches matches the built source, excluding build cache only.
`overlay/` mirrors new files for review; use the complete candidate patch for
composition, including the two narrow C-export/test-import wiring hunks.

Engine identities are external SHA-256 values of the supplied ordered-patch and
build-option preimages, not self-referential source hashes or authentication.
Experimental/default preimages and exact values are in the checkpoint directory.
No final accepted engine profile or canonical application artifact is published.

## Public C boundary

Separate public header `ghostty/vt/shellfox_snapshot_format2.h` and symbols:

```c
GhosttyResult ghostty_shellfox_snapshot_format2_encode_buf(
    GhosttyTerminal source, const GhosttyShellfoxSnapshotFormat2Options *options,
    uint8_t *output, size_t capacity, size_t *out_written,
    GhosttyShellfoxSnapshotFormat2Report *report);

GhosttyResult ghostty_shellfox_snapshot_format2_decode_buf(
    const uint8_t *input, size_t length,
    const GhosttyShellfoxSnapshotFormat2Options *options,
    GhosttyTerminal *out, GhosttyShellfoxSnapshotFormat2Report *report);
```

Sized/versioned options declare expected engine, format/profile/layout and finite
physical/page/row/text/work/allocation/continuation bounds. Compatibility State
and exact runtime config are INSIDE the one atomic blob. No size_t, pointers or
C padding is serialized. The sole profile State is Cora's existing ABI1 type.
Windows x64 actual C/DLL proof gives options size152 and report size64. Header
SHA-256 is `129964605c2839c69cbca2cf65abc30321964c6ddc61579ad71455dea3919156`.

P4's `builtin.is_test` or explicit separate
`-Dshellfox-experimental-format2=true` gate is required. A normal/default build
returns UNSUPPORTED with no terminal/artifact publication. Even the explicit
experimental build leaves the public Legacy constructor unsupported. No live
setter exists. Old budget feature bits do not imply format2 availability.

## Reagan revision changes

- A bounded ALL-node resident-only scan precedes every configuration/profile/
  layout/content accessor. Real compressed history refuses without restoration,
  parser/storage mutation or output writes, followed by an unchanged control suffix.
- An allocation-free complete canonical-table pass rejects distinct IDs with
  equal concrete style/link values BEFORE native references/remap allocations.
  This prevents dedup staging refs from defeating native u16 limits.
- Checked conservative capacity×operation×comparison/copy work is prepaid for
  native source validators/lookups, table insertion, each grapheme/map operation
  and managed cursor restoration. Admission charges it before native allocation.
- Caller buffer exhaustion is OUT_OF_SPACE, encoded/continuation policy exhaustion
  is LIMIT_EXCEEDED with separate internal errors. Mid-buffer/exact boundaries
  and independent policy limits are tested; no failed encode writes caller bytes.
- Supported-build/snapshot/slow-integrity and matching ABI declarations are
  enforced. Managed tables, repeated suffixes, cursor links, saved cursors and
  both-screen replacement have allocation-refusal and late-failure cleanup tests.

These changes need independent review; passed regressions do not settle acceptance.
See REVIEW-002-PLAN.md for the pre-rerun plan and the candidate source manifest.

## Wire and restoration

The approved32-byte prefix is `SFOXSN2\0`, version2, size32, wire-profile2,
explicit layout ABI2, flags0 and exact total encoded length. It is checked before
records/cells/allocation. Strict CRC32C records0x2001..0x2007 carry compatibility,
terminal metadata, screen layout, page layout, cells, continuation and FINISH.
Unknown tags are not skipped; FINISH totals and exact EOF are required.

There is no raw-v1 cell decoder, invalid-codepoint sentinel normalization,
reserved-bit reinterpretation, second terminal parser or duplicate width table.
Unchanged typed metadata/style/charset serializers may be reused; new cells are
explicit pointer-free values with strict scalar/tag/reference checks. Canonical
native-wide/override capture and checked materialization use only P4 helpers.

Page layout captures owned physical columns/rows AND actual capacity columns/
rows separately. Intermediate grow can reduce owned width while retaining a
larger allocation capacity. Every hidden cell and resolved ragged backing length
is retained; all allocation capacity is charged as reservation/work. Physical
stride, not visible width or sum of row lengths, bounds owned slots. Text from
hidden cells and managed tables/strings is admitted too.

The encoder holds exclusive source ownership throughout State/layout/config/
continuation capture and private staging. It never feeds/queries/switches the
source or invokes its callbacks. Failure leaves output bytes untouched and
written0. Decode performs complete framing/metadata/cell/count admission before
native allocation. P4's charged fresh token selects Legacy identity with join
UNKNOWN. Real physical grids are populated under budget, screen geometry then
row/cell semantics are restored through canonical helpers, layout validates,
captured join installs BEFORE continuation, and post-replay State must match.
Only then is the owned passive terminal published. Every partial failure cleans
up. No codec assignment pokes private profile/layout state.

The prototype inherits Native002 canonical runtime restrictions: explicit resize
pull0/1, Legacy image storage0/direct-only, no images/decompression, compressed
source pages, pending APC or unencoded noncanonical sessions/configuration.
The scope bounds codec allocations, not process RSS or later live-input work.

## Actual evidence

Verified project-local Zig0.17.0, WindowsGNU ReleaseSafe/baseline, default features:

- Actual ordered1→2→3→corrected4→5 combined snapshot/Shellfox/format2 build:
  **496 passed, 2 intentional skips**, total498. The skips are Native002's
  and format2's unavailable-build refusal tests on the supported test build.
- Every one of the published P4 oracle's **38 cases /214 cuts** round-trips via
  format2, compares canonical cells/styles/links/graphemes, visible/physical/
  ragged/history storage, current/saved virtual cursors, copy and join, then
  compares the following live input or resize suffix. Both screens are included.
- Targeted Reagan regressions:81 passed/1 supported-build skip. Actual
  snapshot-disabled and Debug/slow-integrity refusal builds each passed74/74.
- All blob truncations, malformed scalars/layout/reference tags, CRC/FINISH/
  trailing data, physical/text/row/page/record/work caps, every64KiB decode
  allocation-refusal boundary and atomic encoder failure are tested.
- Strict public-only C11 consumer linked actual experimental DLL: passed both
  APIs, re-encode/decode, edit+buffer-switch suffix/copy/join, revised mid-buffer/
  encoded/continuation-limit failures, resource failure,
  and continued public-constructor refusal. Actual separate defaultOFF DLL:
  both APIs unsupported/no publication, same constructor refusal.
- Original Native raw-v1 C fixture against frozen Native002 DLL and new
  defaultOFF DLL was byte-identical:
  `f05bfc6439f06562e50a7552e7285503e98493cfab36fc87a14ae687d9b301b8`.

The public consumer is `tests/public-api.c`; it needs mode, declared identity and
an owned codec-generated fixture path. Optional unit-fixture generation uses
`SHELLFOX_FORMAT2_FIXTURE`, explicitly set to a path under project tmp. Normal
unit runs skip only that artifact-generation test when the variable is absent.
No production data, GUI, account traffic or release is involved.

Remaining gates include independent review, broader fuzz/property testing,
matched renderer/formatter/edit/reflow/overflow/replay and wrapper/artifact
acceptance, and all supported-platform native runtime evidence. This scoped
source/codec prototype is not completed Legacy parity or a shipment decision.
