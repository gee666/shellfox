# Layout2 render and interaction review

Status: review evidence, not application acceptance. Public Legacy construction
and default Format2 adoption remain OFF. This review does not change any app,
`sf_cell`, Rust `Cell.width`, SFM1, or default engine contract. Cora owns the
profile-aware producer contract. The proposed versioned transport below still
needs that contract and a checked adapter review.

## Reference and scope

The reference is shipped `@xterm/xterm` 6.0.0, UnicodeV6, DOM renderer. The
standalone fixture pins its actual JS bundle, CSS and relevant source files by
SHA-256. It uses the real parser, buffer, DOM row factory, selection service,
mouse encoder and linkifier. It does not construct replacement terminal cells.

Run from the project root with installed Playwright Chromium:

```text
node crates/ghostty-vt/tests/legacy_render_review.mjs
```

The fixture opens an isolated headless Chromium profile under root `tmp/` and
removes it afterward. URL activation only records a callback. It opens no URL
and uses no account or production traffic.

Passing structural DOM assertions establish span order, attributes, layout
boxes, clipping policy and input coordinate behavior on this host. They do not
establish glyph raster parity, native text shaping, native accessibility, mobile
rendering, or cross-platform application acceptance. Those checks remain pending,
including an exclusive window for actual glyph pixels and native AT.

## Coordinate units must stay separate

| Unit | Meaning | Required use |
| --- | --- | --- |
| Stored grid cell | Physical source column and retained row | Cell lookup, link ranges, selection anchors, source ownership |
| Write advance | How a producer advances its cursor for accepted cell content | Parser/edit/reflow logic, only under the producer's explicit contract |
| Semantic width | xterm `getWidth()`, 0, 1 or 2 | DOM cell eligibility and nominal inline span advance |
| Paint flow column | Accumulated inline semantic advance of rendered source cells | Span, glyph, underline and background position |
| Logical cursor | Source buffer cursor, including pending-wrap/virtual state | State, continuation and cursor classification input |
| Painted cursor | Eligible span after source-grid classification, flow and clipping | Cursor drawing only |
| Mouse grid point | Uniform viewport cell derived from pointer position | Application mouse protocol and link hit testing |
| Selection boundary | Grid boundary after selection rounding and width-zero adjustments | Selection model and copy, not paint flow |

A flat row has one stored entry per physical column. A normal wide lead owns a
semantic width of two, followed by a stored width-zero tail. An orphan wide cell
also has semantic width two, but the next physical entry is not necessarily a
tail. A zero-text cell and an empty orphan stub can both have semantic width zero
while retaining a physical slot. Text, semantic width and physical position alone
must not silently classify one as the other.

P4's `physicalAdvance` returns two for `.wide` and one otherwise, including a
stored tail. That is not the number of entries in a flat cell array. Do not sum
it to discover array offsets or reuse it as DOM paint advance. Cora must name
and document the exact units of the final physical/semantic/kind accessors.

## Paint, backgrounds and clipping

`DomRendererRowFactory.ts:102-107` loads the physical source cell and immediately
skips `width === 0`. This happens before cursor classes, background resolution,
selection attributes, hyperlink hover decoration or span creation. A zero-text
cell keeps its text in the buffer and copy model, but produces no text span and
no background span. An empty orphan stub behaves the same for painting.

The row factory does not skip the following physical entry merely because a
lead has semantic width two. It visits that entry and checks its own width.
Normal tails disappear because their width is zero. An ordinary blank after an
orphan wide cell still creates one cell of flow and can paint its own background.

Rows use inline-block spans, `white-space: pre` and measured letter spacing.
The DOM renderer fixes row width to the visible canvas and sets `overflow:
hidden`. Backgrounds belong to the rendered spans in that order. They are not
independent fills at every stored physical column. A renderer that fills every
physical slot before painting glyphs changes Legacy output after zero cells and
orphan wide cells, even if it correctly exports all text.

The fixture derives these states from actual input and resize operations:

- `SGR41 + U+0301 + SGR0 + A` gives physical cell zero a combining character,
  semantic width zero and red background. The DOM paints A at flow column zero
  and creates no red background span. Cursor grid column two paints near flow
  column one.
- ALT `ABC + SGR41 + 中 + SGR0 + EFG`, shrink to four, overwrite column four
  with X, then grow to eight leaves an empty width-zero stub at physical column
  four. X remains semantic width one. The DOM paints `ABCXEFG`, creates no red
  background span, and does not attach the stub to X.
- ALT `ABCDE中`, shrink to four, grow to six, then blue-background DCH at
  physical column one leaves 中 at physical column four, semantic width two.
  Physical column five is an ordinary width-one blue blank. Its span begins
  near flow column six, not physical column five, at the six-column row clip
  edge. The remainder extends beyond the clip. Fractional CSS rounding can
  leave a small overlap at that edge, so this is not a no-blue-pixels claim.
- Grow that state to eight and write Q at physical column six. The orphan
  remains. Q paints near flow column seven. Treating physical column five as
  an assumed tail would move Q to the wrong place and lose the blue blank.

All indices in those descriptions are zero-based except explicit input CUP
column numbers. Layout boxes can differ by fractional pixel rounding. The test
checks structural relationships rather than exact raster coordinates.

The producer contract still needs explicit visible row length, backing stride,
row traversal, blank handling, span ordering and clip rules. Hidden backing
cells must not leak into visible painting. Do not infer DOM row termination by
summing widths or trimming decoded text yourself.

## Cursor classification is grid-based; drawing is flow-based

`DomRenderer.ts:444` clamps source cursor X with `min(buffer.x, cols - 1)` before
row generation. The row factory classifies the physical source column, but its
early width-zero skip can prevent a cursor span altogether. The fixture moves
the cursor onto both the leading zero-text cell and the retained orphan stub;
the logical cursor exists, but the row has no `.xterm-cursor` element.

For an eligible cell, cursor drawing follows that span's inline position and
semantic extent, then the row clip. A following physical cell is not necessarily
a wide tail. Do not shift to a previous wide lead or guess a cursor box solely
from the old `Cell.width`. Preserve the unmodified logical cursor separately,
including source virtual/pending-wrap state. A projected paint cursor must not
replace it in snapshots, application reports or selection APIs.

## Hyperlinks preserve the existing grid hit policy

There are no clickable anchor elements in the DOM rows. The generated row
container has `pointer-events: none`; actual pointer events reach
`.xterm-screen`. `Linkifier.ts:381-386` obtains uniform viewport grid coordinates
from `MouseService`. `Linkifier.ts:370-375` tests that logical buffer position
against inclusive, one-based provider ranges. Mouse-up activation also checks
that range and the current mouse-down link. It does not use the painted span's
bounds or `event.target` to pick the source cell.

The fixture writes leading U+0301, OSC8-linked A, then unlinked B. A belongs to
physical column one but paints at flow column zero. B belongs to physical column
two and paints at flow column one. Hover/click at paint column zero does not
activate A's link. At uniform grid column one the link activates, even though B
paints there. A's underline remains on A's span at flow column zero. The fixture
records activation without opening the URL.

Preserve that baseline quirk. A paint-span-to-source-owner map is useful for
painting and accessibility review, but it does not authorize visual-span link
hit testing. Nor does it authorize using a paint column as a fake PTY mouse cell.

## Application mouse and selection

`browser/input/Mouse.ts` derives coordinates from the screen rectangle, padding
and cell size. Application mouse coordinates use the uniform grid, bounded to
the viewport, then the selected protocol's numbering and encoding. The fixture
clicks paint column zero in the leading-zero state with modes 1000 and 1006 on.
The terminal emits SGR press/release at one-based column one, not column two
where A is stored. No paint-flow correction belongs in this path.

Selection uses the same grid service with a half-cell rounding offset and an
extra endpoint at `cols + 1` in its one-based intermediate coordinates.
`SelectionService.ts:398-410` converts those coordinates to zero-based retained
buffer coordinates. Its baseline mouse behavior also increments start/end for
*any* width-zero cell at lines 559 and 654. That is a policy quirk, not proof that
all width-zero cells are structural tails. Preserve or separately negotiate it;
do not silently substitute a kind-based selection redesign.

The fixture checks both paths. Programmatically selecting physical cell zero
copies the retained combining character although it never painted. Selecting
physical cell one copies A and returns boundaries `[1, 2)`. A mouse drag from
0.1 to 1.6 uniform cell widths returns that same physical A selection after
rounding and the width-zero start adjustment. Paint positions never become
selection endpoints. Conversion to Ghostty's inclusive endpoint contract must
remain explicit and independently tested.

Native AT text offsets, bounds, hit testing and cursor projection need their own
checked mapping. These DOM fixtures do not establish that acceptance.

## Adapter and SFM2 review requirements

Keep old Native/default `sf_cell`, Rust `Cell.width`, frame layouts and SFM1
semantics unchanged. The current mobile C header explicitly defines width zero
as a wide-cell spacer. Its version-one ABI cannot express the Legacy cases above.
The desktop renderer and iOS renderer also fill backgrounds by physical slot.
Changing only an FFI width accessor would not fix those consumers.

Before Legacy can enter an application path, review a separate, versioned
`sf_cell2`/Rust cell and SFM2 proposal containing at least:

- Explicit engine profile and cell layout identity, with strict unknown-value
  and incompatible-combination rejection.
- Cell kind, semantic width and physical/source ownership. Zero-text, empty stub,
  structural tail and orphan-wide must remain distinguishable. Specify write
  advancement separately from flat storage indexing.
- Visible/backing row geometry and explicit row-flow metadata, including
  traversal, flow offset/extent, skipped-cell behavior, source mapping, row
  termination, background/decorations order and clipping.
- Unmodified logical source-grid cursor and continuation state, plus a separate
  checked cursor paint projection or a fully specified projection rule.
- Separate grid mouse/link hit policy, grid selection boundaries and paint
  metadata. Do not accept a generic coordinate with unspecified units.
- Checked per-frame dimensions, row/cell counts, arithmetic and aggregate text
  budgets. Validate before publishing any partial frame.

Use separate types and versioned entry points. Do not repurpose `Cell.width` or
change the meaning of SFM1 fields under old consumers. Legacy must fail explicitly
at the old adapter/frame boundary unless a consumer has negotiated the new
contract. Current Native002 Rust profile checks and C experimental adoption
already reject Legacy. Retain those checks until the new path is reviewed.

Format2 P5's codec/C proof does not establish an adapter contract, renderer,
interaction, native AT, pixel, or application acceptance. No default constructor,
Format2 decode-to-app adoption, or global model change follows from this review.
