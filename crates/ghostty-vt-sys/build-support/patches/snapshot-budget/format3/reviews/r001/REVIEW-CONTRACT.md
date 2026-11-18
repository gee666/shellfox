# Format3 single joint review contract

Review revision001: FROZEN REVIEW DRAFT, READY for Cora/Maurice cross-review and
Root review. This freezes only this document version, NOT C ABI, wire approval or
implementation permission. No further in-place edits during review. Corrections
require a NEW review revision and published SHA before review resumes.

REVIEW ONLY, except the separately approved external registry V1 bytes and
semantics documented below. C packing, sizeof, header, getter, installer and
codec remain unfrozen and unimplemented. Registry approval does not permit edits
to frozen candidates or their headers. Cora owns private producer state3/transitions;
Maurice owns the matching bounded public-C proposal; Brooklyn owns this one wire
plan. Old P4/P5/P6 candidates, Native002 and all default/public gates remain
unchanged. No constructor, wrapper, app, SFM or release adoption is authorized.

## Accepted private006 prerequisite and review ownership

Root accepts006 ONLY as the reviewed private producer prerequisite for its exact
first subset. Reagan independently closed the URL reset/admission finding and
found no new confirmed blocker in the strict005-to006 delta. The independent
review verified47 touched/1107 composed files, evidence, raw1 and005 immutability;
it DID NOT rerun tests. Candidate checkpoint/README pending-review text remains
immutable historical text, superseded here only by Root's scoped review decision.

Exact006 complete replacement after frozen Native0021/2/3, never stacked on005:

-Patch SHA-256
 `a5c189ee81c4f3e7bd7881c8d758d88e3ef7f62914a67a7f3de999e6906fd103`.
-47-file source digest
 `a1c1b4ea0a540ab1bee5b9f1a7a7e9ef59c31784f07aaaf5b72b5c81582e928b`.

Brooklyn independently rehashed the patch, manifest aggregate, README and
REPRODUCE, and read the actual006 guard/reset source. This contract edit did not
apply/rebase/build006 or rerun tests. Producer522/1skip ReleaseSafe,91 targeted
Debug and15/58 URI-oracle receipts retain their exact candidate scope; Linux is
cross-install only. No general Legacy/current-URL/FullState/platform/public
approval follows. Registry V1 and explicit H preservation remain unchanged.

Brooklyn is sole editor of THIS joint contract. Maurice supplies exact C field/
offset/width/alignment/mask/current-mode, status, getter/budget and install-order
INPUT through Root or a labeled own-tmp artifact, not another wire plan. Cora
cross-reviews actual006 gate/runtime/source facts and the folded tables/install
ledger. Root reviews the one finished contract before any public code/header/
getter/install/codec/constructor/default work. Arithmetic72/232/current88/buffer48
is not authority to infer missing fields or ABI packing.

### Historical005 and004, not accepted admission prerequisites

Private P4-005 is historical CONTRACT REVIEW evidence, NOT an accepted producer
prerequisite. Independent review closed its prior three history/height/failure
mechanisms but found ONE remaining admission gap: DECSTR resets shadow CURRENT
URL fields while the actual native cursor hyperlink survives. A cut with no
print before reset may be admitted, then a following print carries the wrong URI.
The root fix, actual native-URI guard and source following-cell regression are
now closed by independent006 review in the exact scope above. Do not carry005
admission forward as accepted. The immutable005 is a complete replacement after exact frozen Native0021/2/3,
not stacked on004. Patch SHA-256, independently rehashed:
`41ed39b755199fd973a3b22cb8040c53adfed3e4a90bfb0984706d89b44a2dd0`.
The44-file manifest aggregate is
`7ed6493eb22ce76b154bfe9305889b0f0aa33740cb773ccefb617ff17574c1fb`.
Its sorted manifest digest was independently recomputed. This review read the
actual patch/state/guard/row-policy/resize source and checkpoint/README/REPRODUCE;
it did NOT apply/rebase/rebuild005 or independently reproduce all44 source files.

Producer005 reports ReleaseSafe510 passes/1skip, targeted Debug91 passes. Full
Debug exceeded240s and is NOT a pass. Linux cross-install is compilation only,
not runtime proof. New14-case/130-cut oracle reports82 admitted and48 URL-context
StateUnknown cuts. Real GPA96 attempts induced16 failures:8 returned errors,
8 caught-success degradations, plus4 ALT fallback tripwire cases. These are
candidate receipts, not independently accepted general admission or codec proof.

Historical004 remains immutable: patch
`368d8616e747f14fc860cd08378556495326159ac9681272e4f51f7736d51329`,41-file digest
`f128d30f9d19e47eb87fb1de2f3a926ab5f4d337929f8f77221fa0894d997002`.
Its whole-page history prune, full-history height-only savedY and missing loss/
incompatibility mechanisms are closed by independent005 review, not publication
alone. Earlier three cursor/tab findings are closed. No004 admission proof
carries into005;005 still fails the remaining DECSTR/native-URI admission boundary.

The source has Legacy-only owned State with two buffers. Each contains saved
logical_x u32/buffer_y u64/rawfg u32/rawbg u32/symbolic charset/known flag, Tabs
fixed storage and rows_initialized/coordinates_known. Current Pen, four banks,
selector, effective map, fallback and eligibility evidence are separate. Native0
never allocates or consults this state. External registry V1 and exact-H contract
are unchanged. C72/232/current88/buffer48 and wire packing remain joint drafts.

## Source tab facts, not count-only projections

Each buffer owns `Tabs.bits[1025]u64`, exactly8200 bytes. Both buffers therefore
own16400 bytes of tab bit storage regardless of present-key count. Keys are
0..65536 inclusive; per-buffer cardinality may reach65537. The unused tail bits
above65536 are exactly63 unused tail bits and must remain zero. These private
bitset words/padding are not the wire representation.

`maximum_key` is retained HIGH-WATER. remove does not lower it; clear resets it
to0. Current keys do not recover it. SOURCE_STATE3 must carry an explicit tab_high_water u32 beside
actual counted sorted unique keys, for EACH buffer including unfilled ALT.

Direct high-water references in the frozen patch are its zero initializer,
put's max assignment, admitBasic's work prepayment and its bounded key-check
loop. clear resets the whole Tabs value, State.reset resets its enclosing state,
and setup without a start calls clear before adding stops. remove, has,
previous and next do not consult or lower high-water. Copies retain the field.
This source audit does not replace the required exact-preservation fresh-install
and replay proof.

Admit count == actual set cardinality, all keys <= high_water <=65536, and exact
key bounds. `max_tab_column` checks actual keys, NOT high_water. An empty set
with nonzero high_water is valid. Do not require high_water == last present key
or synthesize zero for an empty set.

The producer's basic scan cost is checked `2 + sum(high_water + 1)` for two
buffers, not count. At maximum high-water this is131076 units even if both sets
are empty. Add a separate high-water/domain/work bound; a low present-key limit
does not authorize that scan. Core install must reconstruct zeroed FIXED tab
storage, install all explicit keys, preserve the exact high-water and verify
count/padding. The flat tab array/count denotes actual retained keys only; it
never stands in for high-water or fixed storage. The first contract selects
EXPLICIT high-water preservation. There is no canonicalization alternative in
this plan. The revised logical-buffer fields include high-water and reserved0;
reserved0 must be zero. Never silently sort, truncate, rebuild defaults or lower
high-water.

Setup interval is hardcoded8, not an owned source variable. RUNTIME3 declares
EXACT8 for this subset. Other values reject; no installer may pretend to restore
a configurable interval unsupported by the actual source.

## Version boundary

Proposed distinct40-byte unsigned little-endian prefix:

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 8 | SFOXSN3 plus NUL |
| 8 | 2 | Format version3 |
| 10 | 2 | Prefix size40 |
| 12 | 4 | Codec wire profile3 |
| 16 | 4 | Unchanged cell-layout version2 |
| 20 | 4 | Legacy source/layout-state ABI3 |
| 24 | 4 | New flags0 |
| 28 | 4 | New-prefix reserved0, not old reserved-field reuse |
| 32 | 8 | Exact complete encoded length |

Format3 is Legacy1-only; Native0 is Unsupported and uses its old paths. Wire
profile3 is ONLY a serialization discriminator, not manager-v2/native client-
profile3 identity, mutation/session authority, permission or availability.
No SFOXSN2 fallback, old-relative-Y upgrade, old global-tab expansion or old
reserved/size-field reinterpretation is permitted.

## Joint-review tag roles

These roles are NOT final byte packing or C ABI assignments:

-0x3001 COMPAT3: engine/source/grammar/registry versions and fingerprint, separate
 cell2/state3 identities and sole profile-State ABI1 fields.
-0x3002 RUNTIME3: exact operational line-count/resize policy and fixed interval8,
 distinct from admission budgets. Actual005 requires Terminal-owned exact-row
 policy/config agreement, finite line limit and persistent history-loss gating,
 not merely a line-count setting.006 additionally requires actual empty native
 URI pointer AND ID on every materialized screen. Unsupported
 byte-prune/margin/mixed-resize contexts refuse rather than fabricate source
 history events.
-0x3003 SOURCE_STATE3: BOTH mandatory buffer records, actual lifecycle, retained
 saved u32/u64 coordinates, exact SavedFgBgCharsetV1, counted actual tab keys AND
 retained high-water; separate CURRENT pen/charset/fallback evidence.
-0x3004 TERMINAL3: supported ordinary terminal metadata, no authoritative old
 global-tab bitmap.
-0x3005 SCREEN3: actual materialized geometry/current cursor metadata, not old
 viewport-relative historical saved-Y. Native allocation and source filled state
 remain separate meanings.
-0x3006 PAGE_LAYOUT3: all physical/backing/row/capacity state under cell2 contract.
-0x3007 PAGE_CELLS3: explicitly unchanged cell2 value grammar, not native words.
-0x3008 CONT3: exact replay-safe continuation at the same source cut.
-0x3009 FINISH3: complete record/page/physical/text/source-state/tab/descriptor
 totals and exact EOF.

## Atomic capture, budget scope and codec status

Standalone C metadata read allocates nothing. Its existing live State is borrowed
resident ownership, not a new decoder allocation. Use its local max_work with
actual006 admission plus the exact extra read ledger below. An encoder already
inside a P3 scope must use the same checked internal read path/local limit in
that scope; it must not nest/reset the thread-local guard, refund previous work
or claim a fresh allowance. Public and internal paths require identical values,
refusals and charges. No scope pointer survives the synchronous operation.

The codec adds explicit finite limits for encoded/staged bytes, cumulative
allocation/work, records/pages/rows/visible+backing columns/physical+capacity
slots, text/scalars/string bytes, continuation bytes, source coordinates and
per-buffer/total tabs/descriptor bytes. Zero is zero, not unlimited. Narrow casts,
products and counts are checked before operations. Raw page pool/direct mappings,
whole State and all temporaries remain metered. A metadata getter's success is
not full page/body admission or a whole-lifetime source-work promise.

Encode holds exclusive source ownership for one cut, with no feed/query/switch/
callback. Preflight EVERY node of BOTH materialized native buffers as resident
before validators, page/style/link access or preserved-page routines. Compressed
source refuses without decompression or mutation. Capture/read actual006, sole
join and complete metadata/body under one coherent cut. No old complete layout2
capture/applyRestored/factory call can be used to bypass State3 refusal.

Perform a charged exact-size/validation pass or an equivalent proven staging
algorithm; then allocate private output/scratch through P3 and emit/validate the
COMPLETE artifact. Every repeat pass/lookup/CRC gets another charge. If needed
length exceeds encoded policy, LIMIT_EXCEEDED; if within policy but above caller
capacity, OUT_OF_SPACE. Do not publish a shorter prefix. Recheck complete source
metadata/join/invariants and every poisoned scope before the final prepaid
non-fallible copy. Source pages, compressed state, tabs/H, refs and parser state
remain unchanged on every path. written0 and caller bytes untouched on failure.

Decode borrows immutable input, validates whole framing/body/FINISH/exact EOF
before native construction, then uses the fresh-token phases below. It publishes
one owned terminal only after passive continuation, full actual006 admission,
source/profile/native-context comparison and body validation all pass. Failure
leaves out=NULL and destroys every unpublished resource; it never mutates an
existing live terminal. Do not invoke an old constructor/factory to fill missing
source records or use RIS/DECSTR/URI clearing as a restore repair.

Unsupported version/profile/subset/registry/build/config or ineligible live
source returns UNSUPPORTED. Malformed caller values or malformed wire grammar/
CRC/order/length/reserved/duplicate records return INVALID_VALUE. A proved finite
policy/work/encoded bound returns LIMIT_EXCEEDED; actual backend failure returns
OUT_OF_MEMORY, and an allocation-poisoned scope cannot return success. Source
corrupt-cache UNSUPPORTED and valid-output failure atomicity follow the C read
rules. Native0 and all old formats retain their original paths, not format3
fallback. Codec options/report C packing and symbol/header exposure still require
Root's review; no existing struct/initializer/reserved bytes are reinterpreted.

## Proposed exact wire packing for joint review

This section is Brooklyn's ONE wire proposal, not an ABI freeze or implemented
parser. Cora must cross-review its native auxiliary-state/install implications.
All integers below are explicitly unsigned little-endian. Never memcpy C records,
Zig structs, native enum ordinals, pointers or padding into these payloads.

### Framing and order

After the40-byte prefix, each frame is u16 tag at0, u32 payload_length at2 and
u32 CRC32C at6, followed by payload at10. CRC32C covers the first6 header bytes
then payload, using the same CRC32C convention as experimental002's record.zig.
It is corruption detection, not authentication. Check remaining length, checked
arithmetic, work and record limits before slicing or checksum traversal.

Exactly one COMPAT3, RUNTIME3, SOURCE_STATE3 and TERMINAL3 occur in that order.
Then PRIMARY SCREEN3 and its exact declared PAGE_LAYOUT3/PAGE_CELLS3 pairs;
optional materialized ALT SCREEN3 and its pairs follow. Pages are oldest-first,
include ALL retained history/active/backing rows and never only a visible suffix.
CONT3 occurs once, then FINISH3 once at exact EOF. No optional/unknown tags,
out-of-order pairs, duplicates, missing logical buffers, lazy HISTORY, READY,
trailing bytes or implicit upgrades. Source ALT is mandatory even when native
ALT SCREEN3 is absent. Active ALT requires actual native ALT presence.

### COMPAT3, exactly136 bytes

| Offset | Width | Meaning |
| --- | --- | --- |
| 0 | u32 | Compatibility schema1 |
| 4 | u32 | Sole profile-State ABI1 |
| 8 | u32 | Terminal profile1 |
| 12 | u32 | Cell layout2 |
| 16 | u32 | Legacy source/layout-state ABI3 |
| 20 | u32 | SavedFgBgCharset descriptor ABI1 |
| 24 | u32 | External charset registry V1 |
| 28 | u32 | FIRST subset1 |
| 32 | u32 | Sole canonical preceding_join0/2/3/4/5 |
| 36 | u32 | Flags0 |
| 40 | 64 bytes | Exact lowercase ASCII composed engine-profile digest |
| 104 | 32 bytes | Raw approved registry digest76adfa...7288ae |

The engine-profile material must bind exact Native0021/2/3, accepted private006
prerequisite, final new getter/install/codec sources, build/width/table/config
inputs. The future composed digest is not yet assigned. Prototype888... is not
an accepted identity. Registry bytes alone do not admit a build or state.

### RUNTIME3, exactly40 bytes

| Offset | Width | Meaning |
| --- | --- | --- |
| 0 | u32 | Runtime schema1 |
| 4 | u32 | Policy1, exact Legacy rows |
| 8 | u64 | Actual finite explicit primary history line limit L |
| 16 | u32 | Byte policy0, no operational byte cap |
| 20 | u32 | resize_pull_scrollback0/1 |
| 24 | u32 | Fixed tab interval8 |
| 28 | u32 | Image loading0, direct-only/no external image IO |
| 32 | u64 | Image storage bytes0 |

L must equal source CURRENT, actual native policy and caller-selected runtime;
fit local usize and be<=UINT32_MAX-visibleRows. No native usize unlimited sentinel
is serialized as configuration. Operational L is not a row/allocation admission
budget. Other unsupported common configuration still refuses before capture.

### SOURCE_STATE3, exactly192+4*N bytes

| Offset | Width | Meaning |
| --- | --- | --- |
| 0 | u32 | Source-record wire schema1 |
| 4 | u32 | Fixed wire extent192 |
| 8 | 88 bytes | CURRENT fields in the exact C context field order below |
| 96 | 48 bytes | Primary logical buffer fields in the exact C field order |
| 144 | 48 bytes | ALT logical buffer fields in the exact C field order |
| 192 | N*u32 | Actual sorted primary keys, then sorted ALT keys |

Each scalar is encoded individually in little-endian. This is NOT the232-byte C
Data image: the twelve-word C request/report identity header is intentionally
absent. Identities/join have one wire authority in COMPAT3; active buffer is in
TERMINAL3. Context/record scalar field offsets RELATIVE to each subrecord are
those in Maurice's tables below. No C size/alignment/padding enters the wire.

N=checked sum of both actual tab counts, <=131074. Offsets are in ENTRIES:
primary0, ALT=Nprimary. Exact payload length must be192+4*N; no unused arrays,
embedded bitmap or implicit keys. Per-buffer reserved0, H and retained records
are explicit even with N0. Only CURRENT/saved map tokens1/2/3 and fallback1 are
native-admitted. Source raw BGbit31 or missing/unknown/latent state never becomes
an empty record. Both saved descriptors always exist; their metadata words total
24 bytes, independent of native SavedCursor presence or native style projection.

Source CURRENT geometry/margins/history/resize/mode meanings must agree with
RUNTIME3 and TERMINAL3. The four context bits map explicitly to TERMINAL3 stable
mode meanings: origin bit8, wraparound bit9, reverse_wrap bit15, insert bit1.
Source selector/banks/EFFECTIVE/fallback remain independent of native GL/GR.

### Ordinary terminal and native screen metadata

TERMINAL3 proposes a NEW payload containing the strictly canonical103-byte
Native002 terminal Header, followed immediately by768 original-palette bytes,
32-byte palette mask,3*popcount(mask) overrides, u32 PWD length+bytes and u32 title
length+bytes. The old ceil(columns/8) GLOBAL TAB bitmap is OMITTED, not an
alternative authority. Remaining scalar/enum/color/mode grammar is the unchanged
Native002 snapshot/terminal.zig Header registry, made STRICT: decode/re-encode
must equal original bytes. Invalid enums/presence/reserved bits are rejected,
never normalized to native defaults. Current/default/saved stable mode registries
are byte meanings, not a new native ModePacked bitcast. Require main status,
no unsupported glyph/image state and all common codec/config/cell2 checks.
The primary byte limit header value must be the canonical unlimited-u64 spelling;
finite history limit equals RUNTIME3 L. Screen count1/2 and active key0/1 are exact.

SCREEN3 proposes a64-byte prefix, then an explicitly sized canonical auxiliary
native-metadata block. This auxiliary data preserves ACTUAL native state; it
never substitutes for source saved coordinates/descriptors or charset context.

| Offset | Width | Meaning |
| --- | --- | --- |
| 0 | u32 | Screen schema1 |
| 4 | u32 | Native buffer_id0/1 |
| 8 | u32 | Visible columns |
| 12 | u32 | Physical/backing columns |
| 16 | u32 | Visible rows |
| 20 | u32 | Exact page-pair count, nonzero |
| 24 | u64 | Actual total physical rows, >=visible rows |
| 32 | u32 | Current physical_x |
| 36 | u32 | Current y |
| 40 | u32 | Current pending_wrap0/1 |
| 44 | u32 | Current virtual_overflow0/1 |
| 48 | u32 | Exact auxiliary-native-metadata byte length |
| 52 | u32 | Flags0 |
| 56 | u32 | ACTUAL native saved virtual_overflow0/1,0 if absent |
| 60 | u32 | Reserved0 |

The auxiliary block is strict Native002 SCREEN Header53 bytes, ACTUAL optional
native SavedCursor23 bytes when its present flag is1, empty current-URI kind0
one byte, semantic-seen0/1 one byte, next-image u32 and next-placement u32. Thus
its length is63 or86, with no trailing bytes. Validate all canonical enum/style/
charset/flags by exact re-encoding. Header key/page count/current coordinates/
wrap/history must agree with the new prefix and actual pages. Native saved X/Y
remain native auxiliary fields, not historical sourceY; do not derive either
source saved record from them. Any omission/canonicalization of native auxiliary
state instead requires Cora's explicit equivalence review, not a default here.

Current x/y/wrap/overflow must satisfy cell2 structural constraints; primary
physical columns equal visible, ALT may retain hidden backing. Preserve actual
native presence independently of source rows_initialized. Every materialized
native CURRENT URI pointer/id must be empty by actual006 admission; the serialized
kind0 cannot prove that fact. Historical CELL links remain independently owned.
Synchronize source CURRENT only into the active native pen/effective map through
checked helpers. Do not overwrite dormant native metadata with guessed global
current defaults. No restoration callback/feed/screen switch may manufacture it.

### Page/cell/continuation bodies and FINISH3

PAGE_LAYOUT3 and PAGE_CELLS3 propose the unchanged field-level cell2 wire grammar
in immutable experimental002 P5, SHA
`6572bb5ae16175380ed596b670e15c88347dbe130da8f9c668fcae43c109c340`:
page.zig encodeLayout/describe and cell.zig/tables.zig value encodings ONLY.
This reference adopts no old entrypoint, factory, saved-relative-Y/global tabs,
producer state, decoder implementation or boundedness/admission proof.

PAGE_LAYOUT3 is its22-byte dimension/capacity prefix plus4 bytes per actual row:
u16 backing columns, u8 wrap/continuation/semantic flags, u8 reserved0. It must
match SCREEN3 physical geometry and complete native allocation capacities.
PAGE_CELLS3 contains its strict style/link tables followed by ALL physical cells
and scalar/grapheme suffixes, including backing/padding. Reject invalid padding,
wide/spacer adjacency, references, aliases and unsupported cells BEFORE native
dedup/materialization. Charge every table/capacity/compare/managed operation.
No raw native cell word or raw1 cell normalization enters format3.

CONT3 is the exact bounded passive continuation byte sequence at the captured cut.
It may be empty. Validate grammar/effect safety before replay; captured join is
installed beforehand. Declared empty bytes cannot authorize callbacks or IO.

FINISH3 is exactly96 bytes, twelve u64 totals in this order: records INCLUDING
FINISH, native screens, pages, physical rows, physical cell slots, text scalars,
text bytes, logical buffers2, actual tab keys N, saved descriptors2, saved
metadata bytes24, SOURCE_STATE3 payload bytes192+4*N. Physical slots count ALL
stored size.cols*size.rows, with capacities separately validated/budgeted. Text
scalars count nonzero codepoint/grapheme bases plus suffix scalars. Text bytes
count4 per such scalar plus actual PWD/title/link-URI/explicit-link-ID bytes once
per declared canonical table entry, not native memory size or only visible text.
Any derived copy/UTF8 buffer is a separately charged allocation/work term. Totals
are measured, not trusted allocation hints. Finish and prefix exact length must
agree at exact EOF.

All new field orders/lengths and auxiliary-state choices above require Cora's
integrated cross-review and Root review. They remain ONE proposal, not permission
to implement or freeze a header/parser.

## Saved and current grammar boundary

SavedFgBgCharsetV1 is SOURCE rawfg/rawbg plus actual saved active-charset token.
FG allowed mask is0xffffffff. BG allowed mask is0x7fffffff; BGbit31 rejects.
Preserve P16/P256/raw payload. BG HAS_EXTENDED0x10000000 must not become an FG
mask. Saved/current HAS_EXTENDED in the source BG domain rejects the first
complete-capture subset. Independently reject stored/effective latent extended
or URL state even if its flag is clear. Never zero or normalize it.

The actual current URL model stores known/active booleans, not URI ownership.
The first subset rejects URL state; do not invent a URI string, link ownership
or a complete URL restore contract from those booleans. Existing CELL links
are a separate row-data domain and cannot certify CURRENT parser-pen state.

CURRENT fields are rawfg/rawbg, independently known-empty extended/URL evidence,
selector0..3, four designated symbolic maps, separate effective map and actual
KnownUndefined fallback classification, with modes/margins under exact subset
admission. Effective map cannot be derived from banks[selector]. The private
source has no separate fallback-property-presence boolean; if the public/wire
contract needs that distinction beyond its proven KnownUndefined category, an
actual producer field/proof is required before packing, never a guessed value.

The closed source registry names13 maps; only KnownUndefined/ASCII, DEC Special
and UK have full7-bit native-equivalence proof. Other10 are source-known but
Unsupported for complete native capture. Do not cast private enum ordinals into
public/wire tokens. The approved external registry V1 mapping for the future
joint contract is:

| Token | Source meaning |
| --- | --- |
| 0 | Invalid, never KnownUndefined or an admitted Unknown |
| 1 | KnownUndefined |
| 2 | DEC Special |
| 3 | UK |
| 4 | Dutch |
| 5 | Finnish |
| 6 | French |
| 7 | FrenchCanadian |
| 8 | German |
| 9 | Italian |
| 10 | NorwegianDanish |
| 11 | Spanish |
| 12 | Swedish |
| 13 | Swiss |

Use explicit switches in both directions, never private enum ordinal casts.
Tokens4..13 do not confer native admission. The registry artifact gap is now
closed by independent byte/hash verification against the actual source oracle:

-`registry-v1-proposed-tokens.preimage.txt`:988 bytes, SHA-256
 `76adfa991ecd8eefcc815766ee43a48ace756ee1531634e0650ff53bdd7288ae`.
-`registry-v1-proposed-tokens.review.json`:5071 bytes, SHA-256
 `70ddacfc80d31f1af7babd5810e11aafe0043bffacab2197a43199add344677e`.
-`REGISTRY-PREIMAGE.md`:3738 bytes, SHA-256
 `b7888b4bc135a09cbcb48356d2b2396e10d24bb978cf0556176fff9a2827183d`.

These files are under terminal-profile/prototype/saved-metadata-plan. The frozen
004 patch's actual shellfox_saved3_cases.json was independently extracted in
memory and is byte-identical to oracle.json, SHA-256
`05080e3ed7def47985085f4205b697f6ad72bffd059be32ac14665f1f996a596`.
All13 map hashes were independently recomputed from that fixture's actual maps,
not copied from the proposed token manifest or native charset tables. Key order
was checked against ECMAScript UTF-16 sorting; exact strings and multi-scalar
values were preserved. Compact UTF-8 JSON bytes, without a final LF, produced
the same individual map hashes. Undefined hashes the four bytes `null`.

Regenerating the13 token lines produced byte-for-byte identical988-byte preimage:
`decimal-token:literal-canonical-kind:lowercase-map-sha256` plus LF. There is no
header, BOM, CR or blank line, and there is a final LF. Canonical names include
literal `Undefined`, `DecSpecial`, `Uk`, NOT pretty aliases `KnownUndefined`,
`DEC`, `UK` or private ordinal substitutions. The preimage has no extra domain
header; changing its encoding or adding one would change the reviewed hash.

Registry sub-review is CLOSED after parent, Maurice and Brooklyn independently
verified artifacts and oracle derivation. External registry V1 approves exactly
this mapping and the988-byte preimage/hash above for the future joint contract.
Token0 is invalid; token1 means KnownUndefined, never Unknown or missing data.
Explicit mapping is mandatory; private enum ordinal casts remain forbidden.
Registry approval proves no native equivalence or admission.

Public C packing/header/getter, fresh install and format3 codec remain ON HOLD
for the exact C/getter/install/runtime/body joint contract and Root review.
006's scoped producer review is closed; the prior history/height/failure mechanisms
are closed, but005 is not an accepted
prerequisite. Do not modify frozen candidates to add these tokens. Registry
approval supplies no producer or wider FullState acceptance.
Unsupported commands still execute; no latent state is cleared or eligibility
repaired just to export.

No native SavedCursor/GhosttyStyle/GL/GR/origin/extended clone substitutes for
retained source metadata. Buffer.clear preserves saved records. Missing data,
unknown coordinates/native cells/current context or unproven equivalence refuses.
Do not infer005 support from004 successful cuts. The620/2880 and45/86 counts
previously discussed here are historical004 outcomes, not005 admission proofs.
The005 rerun has its own receipts and refusal counts. Never replace those with
source-known/native-effect equivalence or silently relabel byte-config refusals.

## Actual006 Terminal-owned policy and refusal boundary

A future complete C getter AND encoder MUST call the Terminal-owned
ops.admitBasic guard. State.capture checks local fields only and cannot certify
complete admission. Encoder use of a getter cannot bypass or weaken this guard.
Actual006 guard order distinguishes these cases:

-No owned source3, wrong terminal profile1/layout-state3: Unsupported.
-Incompatible flag or unknown profile state: StateUnknown.
-Primary legacy_history_lost or history above explicit line limit: StateUnknown.
-Primary policy other than legacy_exact_rows, owned/native line-limit mismatch,
 or limit above UINT32_MAX minus current rows: Unsupported.
-Then local saved/current/charset/native-cells eligibility checks apply.
-For EACH materialized primary/alternate native screen, actual cursor.hyperlink
 must be null AND actual cursor.hyperlink_id must be0. Either nonempty operand
 returns Unsupported, even when all shadow URL fields say empty.
-Explicit byte limit must be unlimited at native usize width. Grapheme mode,
 extended reverse-wrap, left/right-margin mode or non-full margins: Unsupported.
-Then independent saved-coordinate/tab limits and actual-H work are checked.

006 reads both pointer/ID operands without dereferencing URI strings, allocating,
repairing shadow state or releasing ownership. Native ALT absence means no native
cursor to inspect, NOT permission to omit its mandatory logical source record.
DECSTR is a proven command transition that ends both materialized CURRENT native
links through typed endHyperlink before resetting the global source current pen.
Historical cell-owned links survive. A getter, encoder, installer or decoder must
NOT run DECSTR/RIS or clear a link to manufacture successful admission.

The native unlimited byte sentinel is a local configuration check, NOT a
platform-sized wire integer. Joint runtime grammar must describe the supported
policy without serializing usize or changing behavior between pointer widths.

Exact row retention now runs inside PageList BEFORE native whole-page pruning.
Ordinary recycle preserves numeric savedY; explicit option trim subtracts actual
source top trim. Height shrink pops below-cursor rows even if styled/nonblank,
then applies capacity top trim and the source savedY transform. Partial option
trim reloads cursor caches from the authoritative pin. A later source trim hook
marks missed correspondence unknown; it cannot repair previously lost rows.
Mixed width+height changes still mark source coordinates unknown.

Resize error, caught ALT removal/recreation/primary fallback and cursor style/
hyperlink degradation invalidate native-cells eligibility. Accidental byte-loss
history remains latched. SGR0, DECSTR, later successful resize or restoring limits
cannot clear loss. Actual full RIS resets applicable state after clearing rows,
while preserving fallback/config that may still refuse. Best-effort later valid
prints continue despite incompatible capture eligibility.

006 is accepted only as this exact private producer prerequisite. The guard is
necessary but does not prove correct fresh materialization or codec equivalence.
Fresh installer/current-mode/refusal semantics must match this actual reviewed
producer before C/body offsets or code can be approved.

## Exact draft CAPI, folded from Maurice INPUT

The following tables and read algorithm are Maurice's section INPUT from
`tmp/state3-capi-006-section-input.md`, folded into this sole contract. They are
proposed NEW definitions, not an accepted ABI or implementation. Cora must
cross-review the integrated tables/ledger. Maurice's final Section9 has been reread and folded, including the named F128
bundles, output-prefix rules and private-cache error classification. Its INPUT
SHA-256 is `abe9bb2607314fd75e1ee5d673aea32c04d614099e8bd294244028f2745f3eec`.
This remains draft pending Cora/Maurice integrated cross-review and Root review.

## 2. Proposed C scalar/layout rules

New types use a deliberate fixed-width `uint32_t size, read_abi` prefix. They do
NOT reuse the old size_t-based structs, their initializer, flags or reserved
bytes. There are no C enum/bool/bitfield/pointer/size_t members in these records.
Function buffer capacities/count outputs remain size_t. Fields called u32/u64
below mean uint32_t/uint64_t, not private Zig enum/struct representations.

### Read options, proposed extent72

| Offset | Width | Field | Required value or meaning |
| --- | --- | --- | --- |
| 0 | u32 | size | Proposed72; exact selected C type size |
| 4 | u32 | read_abi | 1 |
| 8 | u32 | terminal_profile | 1, Legacy only |
| 12 | u32 | profile_state_abi | 1, unchanged canonical State |
| 16 | u32 | cell_layout_version | 2 |
| 20 | u32 | legacy_state_abi | 3, not a cell or client profile |
| 24 | u32 | saved_descriptor_abi | 1, SavedFgBgCharsetV1 |
| 28 | u32 | charset_registry_abi | Approved external registry1 |
| 32 | u32 | subset_id | 1, reviewed FIRST subset |
| 36 | u32 | flags | 0 |
| 40 | u32 | max_saved_x | Historical saved logicalX bound |
| 44 | u32 | max_tab_column | Retained KEY bound, not H |
| 48 | u32 | max_tab_count_per_buffer | Actual retained-key count bound |
| 52 | u32 | reserved | 0 |
| 56 | u64 | max_saved_y | Historical saved bufferY bound |
| 64 | u64 | max_work | Local read-operation budget |

Zero budgets mean zero, never unlimited. Keep checkpoint reproduction limits
65536/1000000/65536/65537 explicit in acceptance tests. Those numerical test
limits are not a general proof for larger admitted operations or source histories.
Hard tab domain is keys/H0..65536 and count0..65537 per buffer. Saved coordinate
limits are independent of present visible dimensions/base/total rows.

### Current context, proposed extent88

| Relative offset | Width | Field |
| --- | --- | --- |
| 0 | u32 | rawfg |
| 4 | u32 | rawbg |
| 8 | u32 | stored_extended, actual value, FIRST requires0 |
| 12 | u32 | url_active, actual boolean, FIRST requires0 |
| 16 | u32 | selector, 0..3 |
| 20 | u32 | effective_charset_token |
| 24 | u32 | fallback_charset_token, FIRST requires1 |
| 28 | u32 | tab_interval, admitted constant8 |
| 32 | u32[4] | designated_charset_tokens, G0..G3 |
| 48 | u32 | current_mode_bits, proposed mask0x0f below |
| 52 | u32 | runtime_resize_pull_scrollback, 0/1 |
| 56 | u32 | visible_columns |
| 60 | u32 | visible_rows |
| 64 | u32 | margin_left |
| 68 | u32 | margin_right |
| 72 | u32 | margin_top |
| 76 | u32 | margin_bottom |
| 80 | u64 | history_line_limit, actual non-null explicit policy |

Current source and URL known flags must be true before success; do not manufacture
known-empty values from the returned zeros. Current URL has only known/active
booleans in006, not URI ownership. Both actual native cursor pointer/ID checks
remain necessary. Source effective charset is NOT designated[selector].

### Logical buffer record, proposed extent48

| Relative offset | Width | Field |
| --- | --- | --- |
| 0 | u32 | buffer_id, 0 primary / 1 alternate |
| 4 | u32 | source_rows_initialized, actual0/1 |
| 8 | u32 | saved_logical_x |
| 12 | u32 | saved_active_charset_token |
| 16 | u64 | saved_buffer_y, retained actual numeric source index |
| 24 | u32 | saved_rawfg |
| 28 | u32 | saved_rawbg |
| 32 | u32 | tab_offset_entries |
| 36 | u32 | tab_count, actual retained keys |
| 40 | u32 | tab_high_water, captured H, NOT last present key |
| 44 | u32 | reserved, 0 |

Both records are mandatory even for unfilled ALT or before explicit save. Native
allocation/presence is a separate SCREEN3 meaning. No saved_present, relative-Y
conversion, clamping or absent-native-SavedCursor default is admitted.

### Complete read data, proposed extent232

The complete table expands Maurice's twelve-header-word declaration without
adding or changing fields:

| Offset | Width | Field | Meaning |
| --- | --- | --- | --- |
| 0 | u32 | size | 232 |
| 4 | u32 | read_abi | 1 |
| 8 | u32 | terminal_profile | 1 |
| 12 | u32 | profile_state_abi | 1 |
| 16 | u32 | cell_layout_version | 2 |
| 20 | u32 | legacy_state_abi | 3 |
| 24 | u32 | saved_descriptor_abi | 1 |
| 28 | u32 | charset_registry_abi | 1 |
| 32 | u32 | subset_id | 1 |
| 36 | u32 | preceding_join | Canonical0/2/3/4/5 |
| 40 | u32 | active_buffer | 0 primary /1 ALT |
| 44 | u32 | flags | 0 |
| 48 | 88 bytes | current | Current-context table above |
| 136 | 48 bytes | primary | Logical buffer0 |
| 184 | 48 bytes | alternate | Logical buffer1 |

Flat keys are primary then alternate, offsets0 and Nprimary in ENTRIES, with
checked total N<=131074. They are actual keys, not a bitmap image.

### Alignment/size status, not ABI certification

The arithmetic model was checked for natural u32 alignment4 and BOTH u64
alignment4 and alignment8. All u64 members are at multiples of8; all proposed
extents are multiples of8. The modeled extents are72/88/48/232 in both classes;
modeled struct alignment is respectively4 or8. No pack pragma or ordinal enum
layout is assumed. This is NOT a compiled target-ABI test or a size freeze.
Before any header is frozen, selected target C compilers must verify sizeof,
alignof and EVERY listed offsetof for Windows/macOS/Linux/Android/iOS target
ABIs. Do not report those compiler checks as already performed.

## 3. Masks and translation domains

SOURCE raw FG allowed mask0xffffffff; SOURCE raw BG0x7fffffff. BGbit31 rejects.
FIRST refuses BG HAS_EXTENDED0x10000000 in saved/current words, independently
nonzero active/latent extended/URL state, unknown metadata and unproven maps.
FGbit28 is valid source underline, NOT a BG HAS_EXTENDED flag. Preserve source
P16/P256 modes and raw payload bits; no native GhosttyStyle projection substitutes
for these records.

Proposed current-mode mask: bit0 origin, bit1 wraparound, bit2 reverse_wrap,
bit3 insert; all other bits reserved0. Each is translated explicitly from the
actual native boolean. This is NOT a native ModeState bitcast. TERMINAL3 owns
other current/default/saved modes; the four duplicated meanings must agree.
Grapheme, extended reverse-wrap and left/right-margin modes remain forbidden by
actual006 admission. Full margins require left/top0, right=cols-1,bottom=rows-1.

Approved registry V1: 0invalid; 1KnownUndefined,2DEC Special,3UK,4Dutch,5Finnish,
6French,7FrenchCanadian,8German,9Italian,10NorwegianDanish,11Spanish,12Swedish,
13Swiss. FIRST only admits1/2/3 and fallback1. Unknown/private ordinals are never
cast into tokens. Registry preimage is exact988B SHA-256
76adfa991ecd8eefcc815766ee43a48ace756ee1531634e0650ff53bdd7288ae.
Its literal source-kind strings and map hashes are in the approved saved-metadata
review artifacts. Registry approval supplies no extra native equivalence.

## 4. Getter behavior and actual006 guard

Proposed symbol: ghostty_shellfox_terminal_source_state3_get, with terminal,
const ReadOptions*, ReadData*, u32*flat_keys, size_t capacity_entries and
size_t*written_entries. Both buffers/current context share one exclusive-owner
cut. No allocation, callback, feed, buffer switching, source page restoration,
string inspection or repair. Success covers these metadata domains, not complete
page/body integrity, rendering, FullState or any client authority.

Mandatory guard is actual006 Terminal ops.admitBasic, never State.capture alone:
owned Source3/profile1/private state revision3; known/non-incompatible profile;
no accidental primary history loss/excess; legacy_exact_rows and exact owned/
native explicit line-limit agreement; limit<=UINT32_MAX-currentRows; field-local
known/subset checks; BOTH pointer==NULL AND hyperlink_id==0 for EACH materialized
native cursor; native byte sentinel unlimited locally; forbidden modes off and
full margins. Canonical profile.capture also validates join State ABI1. These
predicates recheck after continuation. No client can override actual facts.

Keep006's ineligible-native-URI refusal before scan/work behavior, including its
zero-budget refusal test. A getter cannot clear cursor links to gain eligibility.
Reject malformed private H/count domain before invoking a potentially excessive
scan. Corrupt PRIVATE H/count, popcount mismatch, tail bits or key>H are native
ineligibility and return UNSUPPORTED when detected, not caller INVALID_VALUE or
a policy LIMIT_EXCEEDED. Legitimate actual keys beyond a caller key/count bound
are policy LIMIT_EXCEEDED. A malformed caller request remains INVALID_VALUE;
malformed untrusted wire input is not reclassified as a native-cache error.
Source ownership invariants never authorize truncation or cache repair.

Size/null/required-pointer/zero-reserved/zero-flags errors map INVALID_VALUE;
unsupported identity or actual StateUnknown/Unsupported maps UNSUPPORTED;
policy/work maps LIMIT_EXCEEDED; insufficient actual output capacity maps
OUT_OF_SPACE. Unknown ABI/version requests refuse, not fall back. Specify valid,
disjoint options/output/count/source storage as a C precondition. For valid
written storage, errors set written0 and leave metadata/tab bytes unchanged.
NULL written or invalid alias/ranges cannot promise a writable zero slot.

No required-size sizing escape exists. Empty sets/capacity0 may succeed and
still return nonzero H. Validate and stage all fixed metadata, counts/tail/H and
capacity before output writes. Publish flat keys and metadata only after no
fallible step remains; one caller-owned cut prohibits races between phases.

## 5. Exact proposed read-work algorithm

Let Hi be each ACTUAL captured high-water and N=Nprimary+Nalternate.
Actual006 admitBasic charges Wbasic=2+(Hprimary+1)+(Halternate+1), maximum131076.
A low max_tab_column limits KEYS, not H, and does not reduce this charge.

Proposed additional charges: fixed128 contract units; validation pass over1025
words in each buffer; extraction pass over1025 words in each buffer; N key emits.
Thus Wread=Wbasic+128+4100+N, maximum266378 for that algorithm ONLY.
Invoke actual006 admission, then prepay remaining local/global work before the
additional passes; failed reads still consume already charged cumulative work.
Use WORDWISE masks/popcount for bounds/count/tail/H validation. Extraction uses
ctz and clear-lowest-set-bit. Any extra0..H scan or per-key validation/count loop
must have its own charge and changes the bound. This ledger is not implemented
or a claim about CPU latency. Fixed128 must include all proposed scalar work and
be verified against actual implementation before acceptance.

Validate count==popcount, H<=65536, every key<=H, actual key policy and final-word
bits1..63 zero. H may exceed last key or be65536 with N0. Validate reserved0.
No sorting/default setup/key deletion/cache lowering is permitted in the getter.
Caller array memory is distinct from native fixed storage; N*4 describes only
that array and the corresponding explicit wire keys.

## 9. Final read-prefix and fixed-work clarifications

Caller initializes ONLY Data.size=232 and Data.read_abi=1, with full writable/aligned Data storage. Read size FIRST; if wrong, INVALID_VALUE without reading any later Data field. Only after exact size read read_abi; unsupported ABI refuses. No other caller Data byte is read before staging. Data flags/reserved and every buffer reserved word are OUTPUT-ONLY and written0 on success, not caller initialization requirements. Caller may leave other output bytes unspecified. In contrast, Options are input: read size first, read_abi only after exact72, then validate its identities/flags/reserved and all bounds. For valid disjoint written storage set written0 at entry; no other output mutation occurs before commit.

F128 expressly EXCLUDES bounded C pointer/size/identity/precondition safety and the fixed eligibility preflight that must preserve actual006 native-URI Unsupported under zero budget. Do not charge128 before that refusal. Actual admitBasic independently prepays/charges its H scan Wbasic. F128 INCLUDES every POST-GUARD scalar conversion/check, fixed staging initialization and publication/count write. A proposed auditable fixed ledger is58 u32-lanes constructing232B staging ONCE,58 lanes publishing Data ONCE, at most2 lanes publishing size_t written, and10 bounded fixed-decision bundles: canonical Profile State capture; checked N sum; checked size_t conversion; checked N-byte length; capacity comparison; checked additional-work sum; checked total-work sum; local-work-limit comparison; global additional-work charge; final commit bookkeeping. Each construction lane includes its bounded scalar read/closed-token conversion. These are declared accounting groups, not CPU instructions. No zero-then-repopulate staging, second fixed copy, variable lookup or extra postguard pass fits for free. Implementation must demonstrate this bound; otherwise revise F and maximum before acceptance. This is a PROOF OBLIGATION, not a claim that128 has been implemented/proved.

The exact additional ledger is F+2050 validation-word visits+2050 extraction-word visits+N emit steps. Pass1 must use word masks/popcount to prove tail63, actual count, H and key bounds without a per-key loop. A base emit step is ctz, clear-lowest-set-bit, forming the already-validated key and one flat-array store. It does NOT include arbitrary per-key tail/H/bound comparisons, validation or recounting. If those are present, explicitly prepay K for each added comparison/step, so Wread=Wbasic+F+4100+N+K. Any extra0..H scan additionally pays its actual scan length. Maximum266378 is only F128/K0 with the stated two word passes, never an implementation-independent allowance. Prepay additional work BEFORE additional passes/output, and retain separate P3 rounded allocation-work charges. Cora must crossreview the fixed bundles and actual extraction algorithm before Root freeze.

Native-cache versus caller classification is explicit: an established corrupt PRIVATE H/count/popcount/tail/key>H invariant means native ineligibility and maps UNSUPPORTED, never caller INVALID_VALUE. Malformed caller pointers/sizes/flags/reserved fields map INVALID_VALUE; actual caller policy/work exhaustion maps LIMIT_EXCEEDED. Reject out-of-domain cached H/count in bounded safety before any H scan. Remaining cache invariants are proved by the prepaid word pass. Do not promise exhaustive corruption diagnosis before an earlier genuine argument/policy/work refusal, or perform unmetered inspection to choose an error. Actual006 guard-first eligibility, including URI Unsupported under zero budget, remains mandatory. No claim says266378 covers decoder/replay/rounded allocation or its SECOND admission scan; all are separate terms.


## Budget and coherent read requirements

Maurice's minimal new sized/versioned C read proposal must return both fixed
buffer records and separate current context under ONE exclusive-owner capture,
with a bounded caller-owned flat tab array. It must expose actual high-water.
No page restoration, whole-terminal validation per item, source switching,
allocation callback, repair or unbounded sizing allocation is allowed.

Require independent finite historical savedX/savedY bounds (not current cols,
base or total_rows), per-buffer/total tab counts, key/high-water bounds, exact
fixed source/tab storage allowance, descriptor bytes/count and scan/comparison/
allocation work. Empty arrays remain explicit valid sets. Getter failure leaves
metadata and tab bytes untouched and written0; capacity -> OUT_OF_SPACE, policy
-> LIMIT_EXCEEDED, unknown state/unproven subset -> explicit refusal. Exact C
status mapping/field order/sizeof belongs to the joint proposal, not this plan.
The received outer sizes72/232, context88 and logical-buffer48 are arithmetically
coherent review candidates, not frozen headers or offsets. Maurice's exact
field/offset/width/mask/current-mode/size/alignment, status and read-work tables
are now folded above. No inferred packing substitutes for those definitions.
The final qualified Section9 is folded. The joint contract remains pending
Cora/Maurice integrated cross-review and Root review of the wire/install contract.

### Actual006 measured storage and allocator INPUT

Cora's factual INPUT is `tmp/ghostty-profile/joint006-producer-input.json`;
read-only audit source/log are `joint006-size-audit.zig`/`.log` beside it. Brooklyn
read all three. Cora's one WindowsGNU ReleaseSafe Zig0.17.0 audit test passed;
Brooklyn has NOT rerun it. These measurements are private platform facts, not C
packing, wire ABI, universal object sizes or total constructor allocation counts:

| Measured item | Bytes or alignment |
| --- | --- |
| State | 16528 bytes, alignment8 |
| Buffer | 8240 bytes |
| Tabs | 8208 bytes |
| Pen | 16 bytes |
| One bitmap, excluding Tabs metadata | 8200 bytes |
| Two bitmaps, excluding the rest of State | 16400 bytes |

Actual State.create under active P3 heapAllocator charged65536 heap allocation
bytes AND65536 work, mapped0. Alignment8 did not trigger transient overallocation.
It calls reset and both Tabs.setup8, yielding known defaults, NOT fresh UNKNOWN.
The measured65536 work is the allocator charge; it does not independently prove a
complete future constructor/setup/materialization operation ledger. Native
Terminal.init initializes64 inline tab bytes covering512 columns; widths<=512
have NO separate dynamic-tab heap allocation. Above512, dynamic storage is
ceil((cols-512)/8) bytes through the charged allocator. Inline/dynamic zeroing,
reset/setup work and any conditional allocations/replacements remain additional,
as do pages/scratch/other actual constructor allocations.

For the SAME16528-byte request, the source rounding formula yields20480 bytes on
4KiB pages or32768 on16KiB pages. These are arithmetic examples, not non-Windows
runtime proof or permission to assume that private State has that size there.

All source-state ownership/temporary assembly is charged GPA; tab storage costs
16400 bytes plus the rest of the actual state and OS rounding, not count*4.
The actual P3 allocation ledger charges EACH heap/mapped allocation's OS-rounded
bytes to BOTH cumulative allocation total AND work. Windows charges65536-byte
units per allocation; non-Windows uses the runtime page size, including Darwin
16KiB configurations. Do not hardcode4096 or round only the aggregate total.
Overalignment may create charged transient over-allocation. For the actual
wrapped allocator, reservation q is request length n unless alignment a exceeds
runtime page size; then q=2*(n+a), with checked arithmetic. Charge R=ceil(q/g)*g,
where g is the OS granularity above, to allocation total AND work BEFORE the
backend call. Direct-map/pool hooks must ledger their actual reservation path,
not blindly duplicate or omit the wrapper charge. Final tables include every
intermediate request, not merely final retained bytes. Freeing memory does not
erase the work already performed or spent allocation
charges.16400 tab bytes and266378 scan/write work are not a whole-State/GPA/
materialization budget.
Capture must charge scans using the ACTUAL source high-water. Every additional
getter/export scan is charged separately; prepaid admitBasic work does not pay
for a second enumeration. Decoder must charge fixed bitmap initialization,
key insertion, count/padding/high-water validation and subsequent admission
scans even with empty tab arrays. Neither side may substitute key count for
fixed materialization or actual scanned work.

The proposed266378 work ceiling applies ONLY to the stated algorithm: actual
admitBasic high-water scans, bounded1025-word validation/extraction passes per
buffer, key writes and its stated fixed charges. It is not an algorithm-independent
bound. Freeze the exact operation ledger before implementation. Word-based
set-bit enumeration needs verified bounds, including count and tail validation.
Any extraction/count loop that scans0..high-water incurs another separately
prepaid scan. Work refusal must precede output writes or allocation, not rely on
failure partway through publication/materialization.

Required joint getter/install/codec tests:

-Empty sets retaining high-water65536, with fixed storage/work still charged.
-Low max_tab_column and low active keys with high-water65536; do not compare the
 active-key bound to high-water or reduce the scan charge.
-All65537 keys per buffer and combined-count/capacity/checked-arithmetic bounds.
-Exactly63 unused tail bits zero, including rejection of any corrupted tail bit.
-Fresh install and continuation preserve high-water even after key removals;
 explicit clear resets it only when that transition actually occurs.
-Work refusal before output mutation/allocation, including every additional scan.
-Interval values other than8 reject. Entire actual State allocation, GPA rounding
 and fixed16400 bitmap bytes remain charged even for empty sets.

Source live operations also need bounded work/overflow policy. Decoder admission
cannot retroactively bound earlier HTS/setup/reflow work. All size conversions,
sets, descriptors and counters must validate before native allocation or mutation.

### Ledger obligations for the folded joint tables

The final numeric ledger must account for each invocation, not just each distinct
input byte or object. Required phases are source admission, source consistency/
bitmap validation, extraction, wire emit, complete wire admission, native build,
source/profile install, continuation and post-replay admission/comparison. Repeated
frame scans/checksums/validations pay again.266378 cannot cover this entire chain.

-Charge the complete actual owned State allocation and alignment/allocator/OS
 rounding.16400 bytes is only its two bitmap arrays. Track separate live-source
 resident allowance and newly owned decode allocations without silently treating
 already resident storage as free or charging a borrowed pointer as a new alloc.
-Charge metadata/array staging, entire encoded stage, scratch, input/tail copies,
 parser buffers, style/link remaps, palette/strings/graphemes and all error cleanup
 ownership. Bounded fixed stack scratch is declared separately, never an unbounded
 replacement for heap accounting. Caller buffers have explicit capacities and
 input immutability/exclusive ownership requirements.
-Route every new terminal/page/State/managed allocation through the enforced GPA
 and direct-map/pool hooks. Count PageList pooled backing and mmap/VirtualAlloc
 with actual per-allocation OS rounding in BOTH allocation total and work,
 including transient overalignment requests; a heap-only budget is insufficient.
 A pool hit is not free. Allocation work is additional to scan/write work.
-Prepay bitmap zeroing, tail/count/H validation, each word/set-bit visit and key
 write even when sets are empty. Preserve captured H, not maximum surviving key.
-Prepay every managed lookup/insertion/release/comparison from bounded native
 page/table capacity and compared bytes, including style/link alias rejection,
 grapheme append, cursor pen restoration, page growth/split and post-replay scans.
 Never rely on successful deduplication or best-case hash behavior to bound work.
-Prepay record/length/CRC/UTF/scalar decoding, all physical/backing/padding slots,
 continuation validation/replay and all complete metadata/row/copy comparisons.
 Do not allow an early FINISH or declared totals to skip actual validation.
-Work/byte arithmetic is checked before traversal/allocation/mutation it funds.
 Limit failure before publication leaves caller output untouched; deallocation
 and typed ownership cleanup must run on every partially built path. A scope
 poisoned by a backend allocation failure cannot publish success merely because
 a native fallback caught the error or subsequent work happened to succeed.
-Unsupported build/configuration/IO/slow-validation paths without a proven work
 bound refuse. No timer-based promise, decompression, source repair, arbitrary
 callback, external IO or unsupported native fallback substitutes for a bound.

## Fresh internal install and HOLD

Maurice's exact installer-order INPUT is folded here. It defines the required
ordering; the ownership-state summary below does not replace its checks.

## 7. Fresh internal install and continuation ordering

1. Validate whole new framing/identity/records/runtime/source/body/bounds first,
   using borrowed immutable input views. Charge CRC/canonical comparisons and
   every parser validation step. No raw1/format2 fallback or record migration.
2. Require a NEW private fresh-token path. Old complete factories/profile
   installers keep refusing State3; no live/public C setter is added. Force
   charged GPA/TinyIo. Start source/current/saved/coordinates/tabs/native cells
   and join UNKNOWN, not inferred from native constructor/default state.
3. Configure primary legacy_exact_rows with the exact explicit line limit L,
   ALT exact rows/line limit0 where materialized, and no operational byte cap
   BEFORE native body materialization can prune. Materialize the FULL declared
   physical backing without an intermediate prefix trim; never load a partial
   prefix, enforce limits there, then claim the surviving rows are complete.
   L must match source/native/RUNTIME3/caller config, fit usize and be<=UINT32_MAX-
   visibleRows. Interval8 and resize-pull are runtime semantics, not row budgets.
4. Materialize unchanged cell2 body through new checked State3-domain helpers.
   Preserve native geometry/presence separately from source-filled lifecycle.
   Do not resize/feed/switch buffers or later trim to repair native heuristic loss.
5. Initialize both FIXED bitmaps, install ALL explicit validated sorted keys,
   count and CAPTURED H; even empty arrays retain H. Verify all unused tail bits,
   count/H/key invariants. Do not call default setup8 to replace missing data.
6. Install both saved descriptors/lifecycle and independent CURRENT raw words,
   banks/selector/EFFECTIVE/fallback/mode context. Do not invoke restorePen per
   buffer, native SavedCursor/full charset restoration or a constructor default.
   Materialize CURRENT active native pen/effective map via checked helpers.
7. Any error/caught degradation/loss/backend failure permanently fails the fresh
   token. Do not clear latches, invoke RIS/DECSTR, clear a URI, or overwrite known
   flags to make admission pass. Verify BOTH actual native cursor pointer/ID
   contexts empty, including dormant materialized buffers.
8. Once all explicit records/body/native synchronization pass, atomically commit
   source completeness and the sole Profile State ABI1 through the NEW token
   path. Do so BEFORE bounded passive continuation. Old installers remain closed.
9. Replay continuation under all work/allocation/effect bounds. Re-run actual006
   admission and canonical join checks; compare complete source records, both
   tab bitmaps/count/H/lifecycle, current context and body invariants before
   publication. Each replay, admission scan and comparison has a separate charge.
   Failure frees every staged resource exactly once and leaves out=NULL.

Source-specific tab materialization work must include at least2050 word inits,
N insertions,2050 word validations and each actual admission high-water scan.
Post-replay bitmap/key/context comparisons and any temporary expected-data
allocation add their own costs. The post-continuation admitBasic invocation is a
SECOND real H scan, charged from its actual post-replay H even if it differs
from captured H. A prior getter/export/admission prepayment does not pay for it.
Framing, native body constructors/materializers, managed lookups, allocations and
continuation remain separate metered terms. Actual006 Native Tabstops has64 inline
u8 units for512 columns, so fresh widths<=512 have NO separate dynamic-tab heap
allocation. Above512, fresh dynamic storage is ceil((cols-512)/8) u8 units via the
charged allocator. Native init zeroes/setup-resets this object; resize zeroes new
dynamic units and reset zeroes inline/dynamic storage again before its stop loop.
These native GLOBAL tabs are nonauthoritative for Source3 but their actual
initialization, conditional allocations and any later replacements must be
ledgered or explicitly avoided in a reviewed constructor. Never charge a guessed
heap allocation at every width or treat the inline initialization as free.
There is NO universal decoder total of266378 or16400 bytes.


A NEW Cora-owned fresh state3 token is required. The old factory must continue
refusing state3. It forces the charged GPA/TinyIo and makes source3/current/saved/
tabs and join UNKNOWN until explicit complete install. No live/C setter exists.
The proposed linear ownership states, to be matched against Maurice's INPUT, are:

1. ValidatedInput: complete FINISH/exact EOF, records and all bounds checked;
   no native terminal published or allocated by an old constructor.
2. FreshUnknown: private owned terminal under charged factory/policy; no feed,
   callback, source query or public handle can observe it. Constructor defaults
   are staging values only, not captured records or proof of known state.
3. BuiltUnknown: bind exact runtime/policy BEFORE any potentially destructive row
   operation. Materialize ALL declared physical backing without an intermediate
   prefix trim, including builder finish/attach paths. Neither placeholder rows
   nor an old native page heuristic may prune the incoming prefix. Both logical-
   buffer records remain mandatory, independent of native ALT presence or source
   rows_initialized. No default-tab setup may erase captured H.
4. Installed: checked typed source/current/saved/tab/runtime records and sole
   profile/join State installed; all known flags become complete only after every
   dependency passes. Actual native URI ptr/ID emptiness is checked, not repaired.
   Run full006 Terminal admission, layout and cross-record checks before replay.
5. Replayed: bounded validated continuation consumed with captured join installed;
   errors or changed URI/loss/config/subset facts destroy the private token.
6. Publishable: repeat complete admission/layout/source/profile/native-context
   checks and expected cut comparison. Transfer ownership once, only after all
   checks and remaining publication-copy work are prepaid. No partial handle.

All earlier-state failures destroy owned scratch/pages/refs/State through typed
cleanup. No decoder/private field write may manufacture known flags, release URI
ownership or invoke RIS/DECSTR to move a failed state toward Publishable.

Actual006 State.create allocates the full State then reset runs both Tabs.setup8
with known defaults. Native terminal construction initializes global tabs and
conditionally allocates dynamic storage only above512 columns.
Neither operation is a free UNKNOWN constructor. The selected fresh-source direction
is a genuine new UNKNOWN helper avoiding State.create/reset/setup8 as evidence.
Base Terminal construction's conditional global-tab/page costs still exist and
must be ledgered. Any reviewed implementation variation that invokes source
constructor/reset/setup must charge every allocation/zero/setup/replacement
while keeping eligibility unknown. In every case, constructor defaults never
establish captured completeness, and resetting/replacing them has its own work.
Cora supplies actual fixed-State/storage/constructor facts; no constructor code
is authorized by this requirement.

Validate whole new envelope/records/bounds first, materialize physical cell2
storage, then install both exact owned source records/current context via checked
canonical helpers. Preserve fixed tab storage and high-water. Do not run default
setup that deletes latent keys. Install the sole captured profile State after
source/layout completeness but BEFORE continuation. Fresh install and RUNTIME3
must satisfy the retained Terminal-owned conditions AND006's reviewed actual
native-URI guard, not005's incomplete shadow-only check: Source3/profile1/rev3
ownership, no incompatible/unknown/history-loss state, exactLegacyRows,
owned limit equal to actual explicit native line limit, retained history within
limit, limit <= UINT32_MAX-visibleRows, no byte cap, known subset and supported
modes/margins. Wire declarations cannot override the terminal's actual facts or
clear a loss flag. Full RIS is not an installer shortcut or implicit recovery of
unsupported config/fallback.

Recheck complete Terminal-owned admission and compare complete State3, both
saved descriptors/tab sets/lifecycle/current context and join after continuation.
This SECOND admitBasic call incurs its own actual-H scan charge using the H
values then present; a prepaid pre-continuation scan cannot pay for it. All checks
must finish before publication. Prove source-to-wire-to-live behavior including copy/history/
navigation.006's private refusal review is closed, but exact installer ordering,
all materialization/work proofs and Root's joint-contract review remain required.

Current006 has NO such installer or public complete getter. No native/default
saved state may fill that gap. Public C packing/header/getter/install and format3
body/codec stay ON HOLD until the bounded getter/installer/current-mode/mask/
offset/work contract is jointly reviewed by Root. Registry V1 and private006
approval do not lift that hold. Platform/render/wrapper/artifact/AT/app acceptance
and all activation remain open.
